use anchor_lang::prelude::*;

declare_id!("4nMxSRfeW7W2zFbN8FJ4YDvuTzEzCo1e6GzJxJLnDUoZ");

#[program]
pub mod gmsol_mock_chainlink_verifier {

    use super::*;

    pub fn initialize(ctx: Context<Initialize>, user: Pubkey) -> Result<()> {
        ctx.accounts.verifier_account.authority = ctx.accounts.payer.key();
        ctx.accounts.verifier_account.access_controller = ctx.accounts.access_controller.key();
        ctx.accounts.access_controller.user = user;
        Ok(())
    }

    pub fn verify(_ctx: Context<VerifyContext>, compressed_report: Vec<u8>) -> Result<Vec<u8>> {
        use chainlink_data_streams_report::report::decode_full_report;
        use snap::raw::Decoder;

        let mut decoder = Decoder::new();
        let full_report = decoder
            .decompress_vec(&compressed_report)
            .expect("invalid compression");
        let (_, report) = decode_full_report(&full_report).expect("invalid full report");

        cfg_if::cfg_if! {
            if #[cfg(feature = "mock")] {
                Ok(report)
            } else {
                _ = report;
                panic!("The `mock` feature is not enabled");
            }
        }
    }
}

pub const DEFAULT_VERIFIER_ACCOUNT_SEEDS: &[u8; 8] = b"verifier";
pub const DEFAULT_ACCESS_CONTROLLER_ACCOUNT_SEEDS: &[u8] = b"access_controller";

/// Verifier Account.
#[account]
#[derive(InitSpace)]
pub struct VerifierAccount {
    authority: Pubkey,
    access_controller: Pubkey,
}

/// Access Controller.
#[account]
#[derive(InitSpace)]
pub struct AccessController {
    user: Pubkey,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    /// Payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// Verifier Account.
    #[account(
        init,
        payer = payer,
        space = 8 + VerifierAccount::INIT_SPACE,
        seeds = [DEFAULT_VERIFIER_ACCOUNT_SEEDS],
        bump,
    )]
    pub verifier_account: Account<'info, VerifierAccount>,
    /// Access Controller Account.
    #[account(
        init,
        payer = payer,
        space = 8 + VerifierAccount::INIT_SPACE,
        seeds = [DEFAULT_ACCESS_CONTROLLER_ACCOUNT_SEEDS],
        bump,
    )]
    pub access_controller: Account<'info, AccessController>,
    /// The System Program.
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct VerifyContext<'info> {
    /// Verifier Account.
    #[account(
        seeds = [DEFAULT_VERIFIER_ACCOUNT_SEEDS],
        bump,
        has_one = access_controller,
    )]
    pub verifier_account: Account<'info, VerifierAccount>,
    /// Access Controller.
    #[account(has_one = user)]
    pub access_controller: Account<'info, AccessController>,
    /// User.
    pub user: Signer<'info>,
    /// CHECK: Program will validate this based on report input.
    pub config_account: UncheckedAccount<'info>,
}
