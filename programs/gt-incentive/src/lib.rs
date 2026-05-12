use anchor_lang::prelude::*;
use gmsol_store::utils::CpiAuthenticate;
use gmsol_utils::role::RoleKey;

pub mod events;
pub mod instructions;
pub mod states;

pub use events::*;
pub use instructions::*;
pub use states::*;

declare_id!("6iMXVSjiBf75ce9QaUVRnpsUikW8SZm8EaPVGDPfsCso");

#[program]
pub mod gmsol_gt_incentive {
    use super::*;

    /// Initialize the [`AirdropConfig`] singleton.
    ///
    /// # Arguments
    /// - `gov`: The Gov authority address. Must hold the `GT_CONTROLLER` role
    ///   to approve airdrops later.
    #[access_control(CpiAuthenticate::only_admin(&ctx))]
    pub fn initialize_airdrop_config(
        ctx: Context<InitializeAirdropConfig>,
        gov: Pubkey,
    ) -> Result<()> {
        InitializeAirdropConfig::invoke_unchecked(ctx, gov)
    }

    /// Add a new operator or update an existing one in the [`AirdropConfig`].
    #[access_control(CpiAuthenticate::only_admin(&ctx))]
    pub fn update_airdrop_operator(
        ctx: Context<UpdateAirdropOperator>,
        operator: Pubkey,
        timelock_secs: u64,
        max_airdrop_amount: u64,
        is_enabled: bool,
    ) -> Result<()> {
        UpdateAirdropOperator::invoke_unchecked(
            ctx,
            operator,
            timelock_secs,
            max_airdrop_amount,
            is_enabled,
        )
    }

    /// Create a new airdrop. (S1.1)
    pub fn create_airdrop(
        ctx: Context<CreateAirdrop>,
        nonce: [u8; 8],
        duration: u64,
    ) -> Result<()> {
        CreateAirdrop::invoke(ctx, nonce, duration)
    }

    /// Add a recipient to an airdrop's target list. (S1.2)
    pub fn add_airdrop_target(
        ctx: Context<AddAirdropTarget>,
        recipient: Pubkey,
        amount: u64,
    ) -> Result<()> {
        AddAirdropTarget::invoke(ctx, recipient, amount)
    }

    /// Mark an airdrop's target list as complete. (S1.3)
    pub fn complete_airdrop(ctx: Context<CompleteAirdrop>) -> Result<()> {
        CompleteAirdrop::invoke(ctx)
    }

    /// Approve an airdrop. Caller must be Gov AND hold the `GT_CONTROLLER` role. (S1.4)
    #[access_control(CpiAuthenticate::only(&ctx, RoleKey::GT_CONTROLLER))]
    pub fn approve_airdrop(ctx: Context<ApproveAirdrop>) -> Result<()> {
        ApproveAirdrop::invoke_unchecked(ctx)
    }

    /// Claim GT from an approved airdrop. (S1.5)
    pub fn claim_airdrop_target(ctx: Context<ClaimAirdropTarget>) -> Result<()> {
        ClaimAirdropTarget::invoke(ctx)
    }

    /// Cancel an airdrop before approval. Caller must be the original operator.
    ///
    /// Operator authority is enforced by the PDA seeds (a different signer
    /// derives a different PDA), so no store-level access control is required.
    /// Still callable when the operator has been disabled in the config so the
    /// operator always has a path to abandon an in-flight airdrop.
    pub fn cancel_airdrop(ctx: Context<CancelAirdrop>) -> Result<()> {
        CancelAirdrop::invoke(ctx)
    }
}

#[cfg(not(feature = "no-entrypoint"))]
gmsol_utils::security_txt!("GMX-Solana GT Incentive Program");
