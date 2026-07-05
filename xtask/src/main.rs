mod anchor_toml;
mod guardian_set;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Developer tasks for gmx-solana.
#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Wormhole guardian-set clone management for the test validator.
    GuardianSet {
        #[command(subcommand)]
        action: GuardianSetAction,
    },
}

#[derive(Subcommand)]
enum GuardianSetAction {
    /// Fail if the active guardian set is not cloned in Anchor.toml.
    Check,
    /// Rewrite Anchor.toml's managed guardian-set block to the current + previous set.
    Rotate,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::GuardianSet { action } => match action {
            GuardianSetAction::Check => {
                println!("check: not implemented yet");
                ExitCode::SUCCESS
            }
            GuardianSetAction::Rotate => {
                println!("rotate: not implemented yet");
                ExitCode::SUCCESS
            }
        },
    }
}
