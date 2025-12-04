#[allow(unused_imports)]
use std::io::{self, Write};
use std::env;
use std::env::{current_dir, set_current_dir, var_os};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::path::PathBuf;

fn find_all_exes() -> Vec<PathBuf> {
    env::split_paths(&var_os("PATH").unwrap())
        .map(fs::read_dir).flatten().flatten().flatten().filter(
        |entry| entry.metadata().unwrap().permissions().mode() & 0o111 != 0
    ).map(|entry| entry.path()).collect()
}

fn parse_args(args_str: String) -> Vec<String>{
    let mut args = vec![String::from("")];
    let mut ongoing_single_quote = false;
    let mut ongoing_double_quote = false;
    let mut ongoing_escaping = false;
    for elem in args_str.chars() {
        let args_len_curr = args.len();

        if ongoing_escaping {
            args[args_len_curr - 1].push(elem);
            ongoing_escaping = false;
        }
        else if elem == ' ' && !ongoing_single_quote && !ongoing_double_quote {
            if !args[args_len_curr - 1].is_empty() {
                args.push(String::from(""));
            }
        }
        else if elem == '\\' && !ongoing_double_quote {
            ongoing_escaping = true;
        }
        else if elem == '\'' && !ongoing_double_quote {
            ongoing_single_quote = !ongoing_single_quote;
        }
        else if elem == '\"' {
            ongoing_double_quote = !ongoing_double_quote;
        }
        else {
            args[args_len_curr - 1].push(elem);
        }
    }
    if args[args.len() - 1].trim().is_empty() {
        args.pop();
    }
    args
}

fn main() -> io::Result<()> {
    let all_exes = find_all_exes();
    loop {
        print!("$ ");
        io::stdout().flush()?;

        let mut args_str = String::new();
        io::stdin().read_line(&mut args_str)?;
        args_str = args_str[..(args_str.len() - 1)].parse().unwrap();

        // let args: Vec<_> = args_str.split(" ").collect();
        let args = parse_args(args_str);

        let path_matches = all_exes.iter()
            .filter(|entry| *entry.file_stem().unwrap() == *args[0])
            .collect::<Vec<_>>();

        if args[0] == "exit" {
            break;
        }
        else if args[0] == "echo" {
            println!("{}", args[1..].join(" "));
        }
        else if args[0] == "pwd" {
            println!("{}", current_dir()?.display());
        }
        else if args[0] == "cd" {
            let cd_result = set_current_dir(
                args[1].replace("~", var_os("HOME").unwrap().to_str().unwrap())
            );
            if cd_result.is_err() {
                println!("cd: {}: No such file or directory", args[1]);
            }
        }
        else if args[0] == "type" {
            let _path_matches = all_exes.iter()
                .filter(|entry| *entry.file_stem().unwrap() == *args[1])
                .collect::<Vec<_>>();

            if args.len() > 1 {
                if vec!["echo", "exit", "type", "pwd"].contains(&args[1].as_str()) {
                    println!("{} is a shell builtin", args[1]);
                }
                else if _path_matches.first().is_some() {
                    println!("{} is {}", args[1], _path_matches.first().unwrap().display());
                }
                else {
                    println!("{}: not found", args[1]);
                }
            }
        }
        else if path_matches.first().is_some() {
            let aaa = Command::new(&args[0]).args(&args[1..]).spawn();
            aaa.unwrap().wait()?;
        }
        else {
            println!("{}: command not found", args[0]);
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
        let out1 = parse_args(in1);
        assert_eq!(out1, vec!["cat", "/tmp/pig/f\\n53", "/tmp/pig/f\\99", "/tmp/pig/f'\\'38"]);

        let in2 = String::from("cat \"/tmp/fox/f\\n51\" \"/tmp/fox/f\\22\" \"/tmp/fox/f'\\'90\"");
        let out2 = parse_args(in2);
        assert_eq!(out2, vec!["cat", "/tmp/fox/f\\n51",  "/tmp/fox/f\\22", "/tmp/fox/f'\\'90"]);
    }
}