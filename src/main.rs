#[allow(unused_imports)]
use std::io::{self, Write};

fn main() -> io::Result<()> {
    print!("$ ");
    io::stdout().flush()?;

    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    buffer = buffer[..(buffer.len() - 1)].parse().unwrap();

    println!("{}: command not found", buffer);

    Ok(())
}
