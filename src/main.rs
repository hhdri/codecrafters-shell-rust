#[allow(unused_imports)]
use std::io::{self, Write};
use std::env;
use std::env::{current_dir, set_current_dir, var_os};
use std::fs;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::path::PathBuf;

struct Pipeline {
    commands: Vec<PipelineCommand>
}
struct PipelineCommand {
    args_str: String,
    args: Vec<String>,
    in_file: Option<File>,
    out_file: Option<File>,
    err_file: Option<File>,
}

impl PipelineCommand {
    // TODO: make parse_args private and handle unit tests
    pub fn new(args_str: String) -> Self {
        let mut out_file_str: Option<String> = None;
        let mut err_file_str: Option<String> = None;
        let mut out_append = false;
        let mut err_append = false;

        let mut args = vec![String::from("")];
        let mut ongoing_single_quote = false;
        let mut ongoing_double_quote = false;
        let mut elem_idx = 0;
        while elem_idx < args_str.len() {
            let args_len_curr = args.len();
            let elem = args_str.chars().nth(elem_idx).unwrap();

            if elem == ' ' && !ongoing_single_quote && !ongoing_double_quote {
                args.push(String::from(""));
                while args_str.chars().nth(elem_idx).unwrap() == ' ' {
                    elem_idx += 1;
                }
                continue;
            }
            else if elem == '\\' && !ongoing_single_quote {
                elem_idx += 1;
                let candidate_escaper = args_str.chars().nth(elem_idx).unwrap();
                if ongoing_double_quote && !vec!['"', '\\', '$', '`'].contains(&candidate_escaper) {
                    args[args_len_curr - 1].push('\\');
                }
                args[args_len_curr - 1].push(candidate_escaper);
            }
            else if elem == '\'' && !ongoing_double_quote {
                ongoing_single_quote = !ongoing_single_quote;
            }
            else if elem == '\"' && !ongoing_single_quote {
                ongoing_double_quote = !ongoing_double_quote;
            }
            else {
                args[args_len_curr - 1].push(elem);
            }
            elem_idx += 1;
        }
        for i in 0..args.len() - 1 {
            if vec![">", "1>"].contains(&args[i].as_str()) {
                out_file_str = Option::from(args[i + 1].clone());
                for _ in 0..args.len() - i {
                    args.pop();
                }
            }
        }
        for i in 0..args.len() - 1 {
            if vec!["2>"].contains(&args[i].as_str()) {
                err_file_str = Option::from(args[i + 1].clone());
                for _ in 0..args.len() - i {
                    args.pop();
                }
            }
        }
        for i in 0..args.len() - 1 {
            if vec![">>", "1>>"].contains(&args[i].as_str()) {
                out_file_str = Option::from(args[i + 1].clone());
                for _ in 0..args.len() - i {
                    args.pop();
                }
                out_append = true;
            }
        }
        for i in 0..args.len() - 1 {
            if vec!["2>>"].contains(&args[i].as_str()) {
                err_file_str = Option::from(args[i + 1].clone());
                for _ in 0..args.len() - i {
                    args.pop();
                }
                err_append = true;
            }
        }

        let mut out_file: Option<File> = None;
        if out_file_str.is_some() {
            let out_file_err = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(!out_append)
                .append(out_append)
                .open(out_file_str.unwrap());
            out_file = Option::from(out_file_err.unwrap());
        }

        let mut err_file: Option<File> = None;
        if err_file_str.is_some() {
            let err_file_err = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(err_append)
                .append(err_append)
                .open(err_file_str.unwrap());
            err_file = Option::from(err_file_err.unwrap());
        }

        Self {
            args_str,
            args,
            in_file: None,
            out_file,
            err_file
        }

    }
}

impl Pipeline {
    pub fn new(args_str: String) -> Self {
        let commands: Vec<PipelineCommand> = args_str
            .split("|")
            .map(|elem| elem.to_string())
            .map(PipelineCommand::new).collect();
        Self {commands}
    }
}

fn find_all_exes() -> Vec<PathBuf> {
    env::split_paths(&var_os("PATH").unwrap())
        .map(fs::read_dir).flatten().flatten().flatten().filter(
        |entry| entry.metadata().unwrap().permissions().mode() & 0o111 != 0
    ).map(|entry| entry.path()).collect()
}

fn main() -> io::Result<()> {
    let all_exes = find_all_exes();
    loop {
        print!("$ ");
        io::stdout().flush()?;

        let mut args_str = String::new();
        io::stdin().read_line(&mut args_str)?;
        args_str = args_str[..(args_str.len() - 1)].parse().unwrap();

        let mut pipeline = Pipeline::new(args_str);
        let command = &mut pipeline.commands[0];
        let command_arg_0 = command.args[0].clone();

        let path_matches = all_exes.iter()
            .filter(|entry| *entry.file_stem().unwrap() == *command_arg_0)
            .collect::<Vec<_>>();

        if command.args[0] == "exit" {
            break;
        }
        else if command.args[0] == "echo" {
            let mut out_write: Box<dyn Write> = match command.out_file.take() {
                Some(file) => Box::new(file),
                None => Box::new(io::stdout())
            };
            writeln!(out_write, "{}", command.args[1..].join(" "))?;
        }
        else if command.args[0] == "pwd" {
            let mut out_write: Box<dyn Write> = match command.out_file.take() {
                Some(file) => Box::new(file),
                None => Box::new(io::stdout())
            };
            writeln!(out_write, "{}", current_dir()?.display())?;
        }
        else if command.args[0] == "cd" {
            let cd_result = set_current_dir(
                command.args[1].replace("~", var_os("HOME").unwrap().to_str().unwrap())
            );
            if cd_result.is_err() {
                println!("cd: {}: No such file or directory", command.args[1]);
            }
        }
        else if command.args[0] == "type" {
            let _path_matches = all_exes.iter()
                .filter(|entry| *entry.file_stem().unwrap() == *command.args[1])
                .collect::<Vec<_>>();

            if command.args.len() > 1 {
                if vec!["echo", "exit", "type", "pwd"].contains(&command.args[1].as_str()) {
                    let mut out_write: Box<dyn Write> = match command.out_file.take() {
                        Some(file) => Box::new(file),
                        None => Box::new(io::stdout())
                    };
                    writeln!(out_write, "{} is a shell builtin", command.args[1])?;
                }
                else if _path_matches.first().is_some() {
                    let mut out_write: Box<dyn Write> = match command.out_file.take() {
                        Some(file) => Box::new(file),
                        None => Box::new(io::stdout())
                    };
                    writeln!(out_write, "{} is {}", command.args[1], _path_matches.first().unwrap().display())?;
                }
                else {
                    let mut out_write: Box<dyn Write> = match command.out_file.take() {
                        Some(file) => Box::new(file),
                        None => Box::new(io::stdout())
                    };
                    writeln!(out_write, "{}: not found", command.args[1])?;
                }
            }
        }
        else if path_matches.first().is_some() {
            let stdout = match command.out_file.take() {
                Some(file) => Stdio::from(file),
                None => Stdio::from(io::stdout())
            };
            let stderr = match command.err_file.take() {
                Some(file) => Stdio::from(file),
                None => Stdio::from(io::stderr())
            };
            let aaa = Command::new(&command.args[0])
                .args(&command.args[1..])
                .stdout(stdout)
                .stderr(stderr)
                .spawn();
            aaa.unwrap().wait()?;
        }
        else {
            println!("{}: command not found", command.args[0]);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_parse_args() {
        let in1 = String::from("cat \"/tmp/pig/f\\n53\" \"/tmp/pig/f\\99\" \"/tmp/pig/f'\\'38\"");
        let out1 = PipelineCommand::new(in1).args;
        assert_eq!(out1, vec!["cat", "/tmp/pig/f\\n53", "/tmp/pig/f\\99", "/tmp/pig/f'\\'38"]);

        let in2 = String::from("cat \"/tmp/fox/f\\n51\" \"/tmp/fox/f\\22\" \"/tmp/fox/f'\\'90\"");
        let out2 = PipelineCommand::new(in2).args;
        assert_eq!(out2, vec!["cat", "/tmp/fox/f\\n51",  "/tmp/fox/f\\22", "/tmp/fox/f'\\'90"]);

        let in3 = String::from("echo 'hello\\\"worldtest\\\"example'");
        let out3 = PipelineCommand::new(in3).args;
        assert_eq!(out3, vec!["echo", "hello\\\"worldtest\\\"example"]);

        let in4 = String::from("echo \"A \\\\ escapes itself\"");
        let out4 = PipelineCommand::new(in4).args;
        assert_eq!(out4, vec!["echo", "A \\ escapes itself"]);
    }
}