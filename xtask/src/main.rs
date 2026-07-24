mod anchor_toml;
mod guardian_set;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

const ANCHOR_TOML: &str = "Anchor.toml";
const DEFAULT_RPC: &str = "https://api.devnet.solana.com";
// Probe ceiling. Kept well under getMultipleAccounts' 100-address cap; growing it past
// ~100 would require batching detect's RPC call.
const MAX_PROBE: u32 = 15;

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

fn read_anchor_toml() -> eyre::Result<String> {
    std::fs::read_to_string(ANCHOR_TOML)
        .map_err(|e| eyre::eyre!("failed to read {ANCHOR_TOML}: {e}"))
}

fn cmd_check() -> ExitCode {
    let contents = match read_anchor_toml() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let rpc = anchor_toml::validator_url(&contents).unwrap_or_else(|| DEFAULT_RPC.to_string());

    let detected = match guardian_set::detect(&rpc, MAX_PROBE) {
        Ok(d) => d,
        Err(e) => {
            // Soft-pass: a network blip must not masquerade as "rotation needed".
            eprintln!(
                "warning: could not check guardian set ({e}); skipping. \
                       anchor's --clone will surface a real outage."
            );
            return ExitCode::SUCCESS;
        }
    };

    let active_addr = guardian_set::guardian_set_address(detected.active).to_string();
    let cloned = match anchor_toml::uncommented_addresses(&contents) {
        Ok(addrs) => addrs,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if cloned.iter().any(|a| a == &active_addr) {
        println!("guardian set {} is active and cloned; ok.", detected.active);
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "error: Wormhole guardian set {} is active but not cloned in {ANCHOR_TOML}.\n\
             Run `just rotate-guardian-set` to update it.",
            detected.active
        );
        ExitCode::FAILURE
    }
}

fn cmd_rotate() -> ExitCode {
    let contents = match read_anchor_toml() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let rpc = anchor_toml::validator_url(&contents).unwrap_or_else(|| DEFAULT_RPC.to_string());

    let detected = match guardian_set::detect(&rpc, MAX_PROBE) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: could not query guardian sets: {e}");
            return ExitCode::FAILURE;
        }
    };

    let interior = anchor_toml::render_interior(&detected.existing, detected.active, |i| {
        guardian_set::guardian_set_address(i).to_string()
    });
    let updated = match anchor_toml::splice_managed_block(&contents, &interior) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if updated == contents {
        println!(
            "guardian set {} already current; no change.",
            detected.active
        );
        return ExitCode::SUCCESS;
    }
    if let Err(e) = std::fs::write(ANCHOR_TOML, &updated) {
        eprintln!("error: failed to write {ANCHOR_TOML}: {e}");
        return ExitCode::FAILURE;
    }
    let previous = detected.active.saturating_sub(1);
    println!(
        "updated {ANCHOR_TOML}: guardian set {} active, {previous} kept as previous \
         (older commented). Review and commit.",
        detected.active
    );
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::GuardianSet { action } => match action {
            GuardianSetAction::Check => cmd_check(),
            GuardianSetAction::Rotate => cmd_rotate(),
        },
    }
}
