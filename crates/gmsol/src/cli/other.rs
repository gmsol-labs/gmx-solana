use anchor_client::{anchor_lang::system_program, solana_sdk::pubkey::Pubkey};

use crate::GMSOLClient;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Initialize the mock chainlink verifier.
    InitMockChainlinkVerifier,
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: bool,
    ) -> gmsol::Result<()> {
        match self.command {
            Command::InitMockChainlinkVerifier => {
                use mock_chainlink_verifier::{
                    accounts, instruction, DEFAULT_ACCESS_CONTROLLER_ACCOUNT_SEEDS,
                    DEFAULT_VERIFIER_ACCOUNT_SEEDS, ID,
                };

                let chainlink_verifier =
                    Pubkey::find_program_address(&[DEFAULT_VERIFIER_ACCOUNT_SEEDS], &ID).0;
                let access_controller =
                    Pubkey::find_program_address(&[DEFAULT_ACCESS_CONTROLLER_ACCOUNT_SEEDS], &ID).0;

                let rpc = client
                    .store_rpc()
                    .program(ID)
                    .accounts(accounts::Initialize {
                        payer: client.payer(),
                        verifier_account: chainlink_verifier,
                        access_controller,
                        system_program: system_program::ID,
                    })
                    .args(instruction::Initialize { user: *store });

                let req = rpc.into_anchor_request_without_compute_budget();
                crate::utils::send_or_serialize(req, serialize_only, |signature| {
                    println!("{signature}");
                    Ok(())
                })
                .await
            }
        }
    }
}
