use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::doctor;

#[derive(Debug, Parser)]
#[command(name = "appreels", about = "Agent-neutral polished demo-video recorder")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Report capture/render dependency health as JSON.
    Doctor,
    /// Print the demo-script JSON schema.
    Schema {
        #[arg(long)]
        compact: bool,
    },
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Command::Doctor => {
            let report = doctor::report(VERSION, has_command);
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(if report.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Schema { compact } => {
            let schema = appreels_script::script_schema();
            if compact {
                println!("{}", serde_json::to_string(&schema)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&schema)?);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn has_command(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}
