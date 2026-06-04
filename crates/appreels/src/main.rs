mod cli;
mod doctor;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    match cli::run(cli::Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "ok": false,
                    "error": err.to_string(),
                })
            );
            ExitCode::from(1)
        }
    }
}
