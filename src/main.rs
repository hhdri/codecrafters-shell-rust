#[allow(unused_imports)]
use std::io::{self, Write};

fn main() -> io::Result<()> {
    loop {
        print!("$ ");
        io::stdout().flush()?;

        let mut args_str = String::new();
        io::stdin().read_line(&mut args_str)?;
        args_str = args_str[..(args_str.len() - 1)].parse().unwrap();

        let args: Vec<_> = args_str.split(" ").collect();

        if args[0] == "exit" {
            break;
        }
        else if args[0] == "echo" {
            println!("{}", args[1..].join(" "));
        }
        else if args[0] == "type" {
            if args.len() > 1 {
                if vec!["echo", "exit", "type"].contains(&args[1]) {
                    println!("{} is a shell builtin", args[1]);
                }
                else {
                    println!("{}: not found", args[1]);
                }
            }
        }
        else {
            println!("{}: command not found", args[0]);
        }
    }

    Ok(())
}
