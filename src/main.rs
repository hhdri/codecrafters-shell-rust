#[allow(unused_imports)]
use std::io::{self, Write};

fn main() -> io::Result<()> {
    loop {
        print!("$ ");
        io::stdout().flush()?;

        let mut buffer = String::new();
        io::stdin().read_line(&mut buffer)?;
        buffer = buffer[..(buffer.len() - 1)].parse().unwrap();

        if buffer == "exit" {
            break;
        }
        else {
            println!("{}: command not found", buffer);
        }
    }

    Ok(())
}
