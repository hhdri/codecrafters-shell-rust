#[allow(unused_imports)]
use std::io::{self, Write};
use std::env;
use std::env::{current_dir, set_current_dir};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::path::PathBuf;

fn find_all_exes() -> Vec<PathBuf> {
    env::split_paths(&env::var_os("PATH").unwrap())
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

        let args: Vec<_> = args_str.split(" ").collect();

        let path_matches = all_exes.iter()
            .filter(|entry| entry.file_stem().unwrap() == args[0])
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
            let cd_result = set_current_dir(args[1]);
            if cd_result.is_err() {
                println!("cd: {}: No such file or directory", args[1]);
            }
        }
        else if args[0] == "type" {
            let _path_matches = all_exes.iter()
                .filter(|entry| entry.file_stem().unwrap() == args[1])
                .collect::<Vec<_>>();

            if args.len() > 1 {
                if vec!["echo", "exit", "type", "pwd"].contains(&args[1]) {
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
            let aaa = Command::new(args[0]).args(&args[1..]).spawn();
            aaa.unwrap().wait()?;
        }
        else {
            println!("{}: command not found", args[0]);
        }
    }

    Ok(())
}
