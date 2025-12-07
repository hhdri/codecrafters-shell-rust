use std::io::{self, pipe, PipeReader, PipeWriter, Write};
use std::{env, thread};
use std::env::{current_dir, set_current_dir, var_os};
use std::fs;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::path::PathBuf;
use std::thread::JoinHandle;
use rustyline::error::ReadlineError;
use rustyline::{Result, Editor, Context, CompletionType};
use rustyline::completion::{Completer, Pair};
use rustyline::config::Configurer;
use rustyline::history::DefaultHistory;
use rustyline_derive::{Helper, Highlighter, Hinter, Validator};

struct Pipeline {
    commands: Vec<PipelineCommand>
}
struct PipelineCommand {
    args: Vec<String>,
    out_file: Option<File>,
    err_file: Option<File>,
    in_pipe: Option<PipeReader>,
    out_pipe: Option<PipeWriter>,
}

impl PipelineCommand {
    fn open_write_file(filename: Option<String>, append: bool) -> Option<File> {
        filename.map(|f|
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(!append)
                .append(append)
                .open(f)
                .expect("file can't be opened for writing"),
        )
    }
    pub fn run(&mut self) -> io::Result<()> {
        let all_exes = find_all_exes();
        let path_matches = all_exes.iter()
            .filter(|entry| *entry.file_stem().unwrap() == *self.args[0])
            .collect::<Vec<_>>();

        match self.args[0].as_str() {
            "echo" => {
                writeln!(self.get_out_write(), "{}", self.args[1..].join(" "))?;
            }
            "pwd" => {
                writeln!(self.get_out_write(), "{}", current_dir()?.display())?;
            }
            "cd" => {
                let cd_result = set_current_dir(
                    self.args[1].replace("~", var_os("HOME").unwrap().to_str().unwrap())
                );
                if cd_result.is_err() {
                    println!("cd: {}: No such file or directory", self.args[1]);
                }
            }
            "type" => {
                let _path_matches = all_exes.iter()
                    .filter(|entry| *entry.file_stem().unwrap() == *self.args[1])
                    .collect::<Vec<_>>();

                if self.args.len() > 1 {
                    if matches!(self.args[1].as_str(), "echo" | "exit" | "type" | "pwd" | "cd") {
                        writeln!(self.get_out_write(), "{} is a shell builtin", self.args[1])?;
                    }
                    else if let Some(path) =  _path_matches.first() {
                        writeln!(self.get_out_write(), "{} is {}", self.args[1], path.display())?;
                    }
                    else {
                        writeln!(self.get_out_write(), "{}: not found", self.args[1])?;
                    }
                }
            }
            _ if path_matches.first().is_some() => {
                let stdin = self.in_pipe.take()
                    .map(Stdio::from)
                    .unwrap_or(Stdio::inherit());
                let stdout = self.out_pipe.take().map(Stdio::from).unwrap_or(
                    self.out_file.take()
                        .map(Stdio::from)
                        .unwrap_or(Stdio::from(io::stdout()))
                );
                let stderr = self.err_file.take()
                    .map(Stdio::from)
                    .unwrap_or(Stdio::from(io::stderr()));

                let child = Command::new(&self.args[0])
                    .args(&self.args[1..])
                    .stdout(stdout)
                    .stderr(stderr)
                    .stdin(stdin)
                    .spawn();

                child.unwrap().wait()?;
            }
            _ => eprintln!("{}: command not found", self.args[0])
        };

        Ok(())
    }
    pub fn get_out_write(&mut self) -> Box<dyn Write> {
        match self.out_pipe.take() {
            Some(out_pipe) => Box::new(out_pipe),
            _ => {
                self.out_file.take()
                    .map(|f| Box::new(f) as Box<dyn Write>)
                    .unwrap_or_else(|| Box::new(io::stdout()))
            }
        }
    }
    pub fn new(args_str: &str, in_pipe: Option<PipeReader>, out_pipe: Option<PipeWriter>) -> Self {
        let mut args = vec![String::from("")];
        let mut ongoing_single_quote = false;
        let mut ongoing_double_quote = false;
        let mut chars = args_str.chars().peekable();
        let mut opt_c = chars.next();
        loop {
            let Some(mut c) = opt_c else { break };

            let args_len_curr = args.len();

            if c == ' ' && !ongoing_single_quote && !ongoing_double_quote {

                args.push(String::from(""));
                while c == ' ' {
                    opt_c = chars.next();
                    match opt_c {
                        Some(next_c) => c = next_c,
                        _ => break
                    }
                }

                continue;
            }
            else if c == '\n' { }
            else if c == '\\' && !ongoing_single_quote {
                opt_c = chars.next();
                match opt_c {
                    Some(candidate_escaper) => {
                        if ongoing_double_quote && !matches!(candidate_escaper, '"' | '\\' | '$' | '`') {
                            args[args_len_curr - 1].push('\\');
                        }
                        args[args_len_curr - 1].push(candidate_escaper);
                    },
                    _ => break
                }
            }
            else if c == '\'' && !ongoing_double_quote {
                ongoing_single_quote = !ongoing_single_quote;
            }
            else if c == '\"' && !ongoing_single_quote {
                ongoing_double_quote = !ongoing_double_quote;
            }
            else {
                args[args_len_curr - 1].push(c);
            }

            opt_c = chars.next();
        }

        let mut out_file_str: Option<String> = None;
        let mut err_file_str: Option<String> = None;
        let mut out_append = false;
        let mut err_append = false;
        let mut n_pop = 0;
        for i in 0..args.len() - 1 {
            if matches!(args[i].as_str(), ">" | "1>" | ">>" | "1>>") {
                out_file_str = Some(args[i + 1].clone());
                n_pop = n_pop.max( args.len() - i);
                out_append = matches!(args[i].as_str(), ">>" | "1>>");
            }
            if matches!(args[i].as_str(), "2>" | "2>>") {
                err_file_str = Some(args[i + 1].clone());
                n_pop = n_pop.max(args.len() - i);
                err_append = args[i].as_str() == "2>>";
            }
        }
        for _ in 0..n_pop { args.pop(); }

        Self {
            args,
            out_file: Self::open_write_file(out_file_str, out_append),
            err_file: Self::open_write_file(err_file_str, err_append),
            in_pipe,
            out_pipe,
        }

    }
}

impl Pipeline {
    pub fn new(args_str: &str) -> Self {
        let commands_str: Vec<_> = args_str.split("|").collect();
        let n_commands = commands_str.len();
        let mut pipes_in: Vec<Option<PipeReader>> = vec![];
        let mut pipes_out: Vec<Option<PipeWriter>> = vec![];
        for _ in 0..n_commands - 1 {
            let (_pipe_in, _pipe_out) = pipe().expect("can't create pipes between processes");
            pipes_in.push(Some(_pipe_in));
            pipes_out.push(Some(_pipe_out));
        }
        let mut commands: Vec<PipelineCommand> = vec![];
        for i in 0..n_commands {
            commands.push(PipelineCommand::new(
                commands_str[i].trim(),
                if i == 0 {None} else {pipes_in[i-1].take()},
                if i == n_commands - 1 {None} else {pipes_out[i].take()}
            ))
        }
        Self {commands}
    }
}

fn find_all_exes() -> Vec<PathBuf> {
    env::split_paths(&var_os("PATH").unwrap())
        .filter_map(|p| fs::read_dir(p).ok())
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|entry| entry.metadata().unwrap().permissions().mode() & 0o111 != 0)
        .map(|entry| entry.path())
        .collect()
}

#[derive(Helper, Highlighter, Hinter, Validator)]
struct CommandCompleter {
    commands: Vec<String>,
}

impl Completer for CommandCompleter {
    type Candidate = Pair;

    fn complete(&self, line: &str, _: usize, _ctx: &Context) -> Result<(usize, Vec<Pair>)> {
        let mut candidates = Vec::new();

        for cmd in &self.commands {
            if cmd.starts_with(line) {
                candidates.push(Pair {
                    display: cmd.clone(),
                    replacement: cmd.clone() + " ",
                });
            }
        }

        Ok((0, candidates))
    }
}

fn main() -> io::Result<()> {
    // TODO: Handle builtins in a more structured way
    let builtins = ["echo", "exit", "type", "pwd", "cd"];
    let mut commands: Vec<String> = find_all_exes().iter()
        .filter_map(|path| path.file_stem().and_then(|s| s.to_str()))
        .map(String::from)
        .chain(builtins.iter().map(|&s| s.to_string()))
        .collect();
    commands.sort();
    let helper = CommandCompleter { commands };
    let mut rl: Editor<CommandCompleter, DefaultHistory> = Editor::new().unwrap();
    rl.set_helper(Some(helper));
    rl.set_completion_type(CompletionType::List);


    loop {
        let args_str = rl.readline("$ ");

        match args_str {
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                continue;
            },
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            },
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            },
            Ok(_) => {}
        }

        let pipeline = Pipeline::new(args_str.unwrap().as_str());

        let mut join_handles: Vec<JoinHandle<()>> = vec![];
        for mut pipeline_command in pipeline.commands {
            if pipeline_command.args[0] == "exit" {
                return Ok(())
            }
            else {
                let thread_join_handle = thread::spawn(move || {
                    pipeline_command.run().expect("failed to run command")
                });
                join_handles.push(thread_join_handle);
            }
        }
        for join_handle in join_handles {
            join_handle.join().expect("failed to run pipeline part");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args() {
        let in1 = "cat \"/tmp/pig/f\\n53\" \"/tmp/pig/f\\99\" \"/tmp/pig/f'\\'38\"";
        let out1 = PipelineCommand::new(in1, None, None).args;
        assert_eq!(out1, vec!["cat", "/tmp/pig/f\\n53", "/tmp/pig/f\\99", "/tmp/pig/f'\\'38"]);

        let in2 = "cat \"/tmp/fox/f\\n51\" \"/tmp/fox/f\\22\" \"/tmp/fox/f'\\'90\"";
        let out2 = PipelineCommand::new(in2, None, None).args;
        assert_eq!(out2, vec!["cat", "/tmp/fox/f\\n51",  "/tmp/fox/f\\22", "/tmp/fox/f'\\'90"]);

        let in3 = "echo 'hello\\\"worldtest\\\"example'";
        let out3 = PipelineCommand::new(in3, None, None).args;
        assert_eq!(out3, vec!["echo", "hello\\\"worldtest\\\"example"]);

        let in4 = "echo \"A \\\\ escapes itself\"";
        let out4 = PipelineCommand::new(in4, None, None).args;
        assert_eq!(out4, vec!["echo", "A \\ escapes itself"]);
    }
}