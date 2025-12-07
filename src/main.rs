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

#[derive(Copy, Clone)]
pub enum Builtin {
    Echo, Pwd, Cd, Type
}

impl Builtin {
    pub const ALL: &'static [(&'static str, Builtin)] = &[
        ("echo", Builtin::Echo),
        ("pwd", Builtin::Pwd),
        ("cd", Builtin::Cd),
        ("type", Builtin::Type),
    ];

    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.iter().find(|(name, _)| *name == s).map(|(_, b)| *b)
    }

    pub fn names() -> impl Iterator<Item = &'static str> {
        Self::ALL.iter().map(|(name, _)| *name)
    }
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
    fn builtin_echo(&mut self) -> io::Result<()> {
        writeln!(self.get_out_write(), "{}", self.args[1..].join(" "))?;
        Ok(())
    }
    fn builtin_pwd(&mut self) -> io::Result<()> {
        writeln!(self.get_out_write(), "{}", current_dir()?.display())?;
        Ok(())
    }
    fn builtin_cd(&mut self) -> io::Result<()> {
        let cd_result = set_current_dir(
            self.args[1].replace("~", var_os("HOME").unwrap().to_str().unwrap())
        );
        if cd_result.is_err() {
            writeln!(self.get_err_write(), "cd: {}: No such file or directory", self.args[1])?;
        }
        Ok(())
    }
    fn builtin_type(&mut self, all_exes: &Vec<PathBuf>) -> io::Result<()> {
        let _path_matches = all_exes.iter()
            .filter(|entry| *entry.file_stem().unwrap() == *self.args[1])
            .collect::<Vec<_>>();

        if self.args.len() > 1 {
            if Builtin::from_str(self.args[1].as_str()).is_some() || self.args[1] == "history" || self.args[1] == "exit" {
                writeln!(self.get_out_write(), "{} is a shell builtin", self.args[1])?;
            }
            else if let Some(path) =  _path_matches.first() {
                writeln!(self.get_out_write(), "{} is {}", self.args[1], path.display())?;
            }
            else {
                writeln!(self.get_out_write(), "{}: not found", self.args[1])?;
            }
        }
        Ok(())
    }
    fn external_command(&mut self) -> io::Result<()> {
        Command::new(&self.args[0])
            .args(&self.args[1..])
            .stdout(
                self.out_pipe.take().map(Stdio::from).unwrap_or(
                    self.out_file.take()
                        .map(Stdio::from)
                        .unwrap_or(Stdio::from(io::stdout()))
                ))
            .stderr(self.err_file.take()
                .map(Stdio::from)
                .unwrap_or(Stdio::from(io::stderr())))
            .stdin(self.in_pipe.take()
                .map(Stdio::from)
                .unwrap_or(Stdio::inherit()))
            .spawn()
            .expect("failed to run external command")
            .wait()?;

        Ok(())
    }
    pub fn run(&mut self) -> io::Result<()> {
        let all_exes = find_all_exes();
        let path_matches = all_exes.iter()
            .filter(|entry| *entry.file_stem().unwrap() == *self.args[0])
            .collect::<Vec<_>>();

        // Echo, Pwd, Cd, Type, History, Exit,
        match Builtin::from_str(self.args[0].as_str()) {
            Some(Builtin::Echo) => self.builtin_echo()?,
            Some(Builtin::Pwd) => self.builtin_pwd()?,
            Some(Builtin::Cd) => self.builtin_cd()?,
            Some(Builtin::Type) => self.builtin_type(&all_exes)?,
            None => match self.args[0].as_str() {
                _ if path_matches.first().is_some() => self.external_command()?,
                _ => eprintln!("{}: command not found", self.args[0])
            }
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
    pub fn get_err_write(&mut self) -> Box<dyn Write> {
        self.out_file.take()
            .map(|f| Box::new(f) as Box<dyn Write>)
            .unwrap_or_else(|| Box::new(io::stdout()))
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
    let mut commands: Vec<String> = find_all_exes().iter()
        .filter_map(|path| path.file_stem().and_then(|s| s.to_str()))
        .map(String::from)
        .chain(Builtin::names().map(|e| e.to_string()))
        .chain(vec!["history".to_string(), "exit".to_string()])
        .collect();
    commands.sort();
    let helper = CommandCompleter { commands };
    let mut rl: Editor<CommandCompleter, DefaultHistory> = Editor::new().unwrap();
    rl.set_helper(Some(helper));
    rl.set_completion_type(CompletionType::List);

    let mut history: Vec<String> = vec![];


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
            Ok(args_str) => {history.push(args_str)}
        }

        let pipeline = Pipeline::new(history.last().unwrap().as_str());

        let mut join_handles: Vec<JoinHandle<()>> = vec![];
        for mut pipeline_command in pipeline.commands {
            if pipeline_command.args[0] == "exit" {
                return Ok(())
            }
            else if pipeline_command.args[0] == "history" {
                let mut last_n = history.len();
                if pipeline_command.args.len() > 1 {
                    if let Some(_last_n) = pipeline_command.args[1].parse::<usize>().ok() {
                        last_n = _last_n;
                    }
                }
                for (idx, elem) in history.iter().enumerate() {
                    if last_n < history.len() { last_n += 1 }
                    else { println!("    {}  {}", idx + 1, elem) }
                }
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