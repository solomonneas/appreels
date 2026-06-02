mod cli;
mod doctor;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    match cli::run(cli::Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("appreels: {err}");
            ExitCode::from(1)
        }
    }
}
