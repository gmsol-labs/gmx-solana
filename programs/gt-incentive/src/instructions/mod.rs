use anchor_lang::prelude::*;
use gmsol_store::{
    cpi::{accounts::MintGtReward, mint_gt_reward},
    program::GmsolStore,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::events::{
    AirdropApproved, AirdropCancelled, AirdropClaimed, AirdropCompleted, AirdropCreated,
    AirdropOperatorUpdated, AirdropTargetAdded,
};
use crate::states::{Airdrop, AirdropConfig, AirdropTarget, GT_AUTHORITY_SEED};

// ============================================================================
// 1. initialize_airdrop_config (admin)
// ============================================================================

/// Accounts definition for [`initialize_airdrop_config`](crate::gmsol_gt_incentive::initialize_airdrop_config).
#[derive(Accounts)]
pub struct InitializeAirdropConfig<'info> {
    /// The admin authority. Must be the store admin (verified via CPI).
    #[account(mut)]
    pub authority: Signer<'info>,
    /// The store this airdrop config belongs to.
    /// CHECK: validated by `CpiAuthenticate::only_admin` via CPI to store.
    pub store: UncheckedAccount<'info>,
    /// The airdrop config PDA to be created.
    #[account(
        init,
        payer = authority,
        space = 8 + AirdropConfig::INIT_SPACE,
        seeds = [AirdropConfig::SEED, store.key().as_ref()],
        bump,
    )]
    pub airdrop_config: AccountLoader<'info, AirdropConfig>,
    /// The store program (for CPI admin check).
    pub store_program: Program<'info, GmsolStore>,
    pub system_program: Program<'info, System>,
}

impl InitializeAirdropConfig<'_> {
    /// CHECK: only the store admin is allowed to invoke (enforced via `access_control`).
    pub(crate) fn invoke_unchecked(ctx: Context<Self>, gov: Pubkey) -> Result<()> {
        let mut config = ctx.accounts.airdrop_config.load_init()?;
        config.init(ctx.bumps.airdrop_config, &ctx.accounts.store.key(), &gov)?;
        Ok(())
    }
}

impl<'info> WithStore<'info> for InitializeAirdropConfig<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }
    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for InitializeAirdropConfig<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }
    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

// ============================================================================
// 2. update_airdrop_operator (admin)
// ============================================================================

/// Accounts definition for [`update_airdrop_operator`](crate::gmsol_gt_incentive::update_airdrop_operator).
#[derive(Accounts)]
pub struct UpdateAirdropOperator<'info> {
    pub authority: Signer<'info>,
    /// CHECK: validated by `CpiAuthenticate::only_admin` via CPI to store.
    pub store: UncheckedAccount<'info>,
    #[account(
        mut,
        constraint = airdrop_config.load()?.is_initialized() @ CoreError::AirdropConfigNotInitialized,
        constraint = airdrop_config.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [AirdropConfig::SEED, store.key().as_ref()],
        bump = airdrop_config.load()?.bump,
    )]
    pub airdrop_config: AccountLoader<'info, AirdropConfig>,
    pub store_program: Program<'info, GmsolStore>,
}

impl UpdateAirdropOperator<'_> {
    /// CHECK: only the store admin is allowed to invoke.
    pub(crate) fn invoke_unchecked(
        ctx: Context<Self>,
        operator: Pubkey,
        timelock_secs: u64,
        max_airdrop_amount: u64,
        is_enabled: bool,
    ) -> Result<()> {
        {
            let mut config = ctx.accounts.airdrop_config.load_mut()?;
            config.upsert_operator(&operator, timelock_secs, max_airdrop_amount, is_enabled)?;
        }
        emit!(AirdropOperatorUpdated::new(
            ctx.accounts.store.key(),
            ctx.accounts.authority.key(),
            operator,
            timelock_secs,
            max_airdrop_amount,
            is_enabled,
        )?);
        Ok(())
    }
}

impl<'info> WithStore<'info> for UpdateAirdropOperator<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }
    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for UpdateAirdropOperator<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }
    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

// ============================================================================
// 3. create_airdrop (operator) - S1.1
// ============================================================================

/// Accounts definition for [`create_airdrop`](crate::gmsol_gt_incentive::create_airdrop).
#[derive(Accounts)]
#[instruction(nonce: [u8; 8])]
pub struct CreateAirdrop<'info> {
    /// The operator creating this airdrop, who pays for the new account.
    #[account(mut)]
    pub operator: Signer<'info>,
    /// CHECK: only used to scope the airdrop to a specific store.
    pub store: UncheckedAccount<'info>,
    #[account(
        constraint = airdrop_config.load()?.is_initialized() @ CoreError::AirdropConfigNotInitialized,
        constraint = airdrop_config.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [AirdropConfig::SEED, store.key().as_ref()],
        bump = airdrop_config.load()?.bump,
    )]
    pub airdrop_config: AccountLoader<'info, AirdropConfig>,
    #[account(
        init,
        payer = operator,
        space = 8 + Airdrop::INIT_SPACE,
        seeds = [Airdrop::SEED, store.key().as_ref(), operator.key().as_ref(), &nonce],
        bump,
    )]
    pub airdrop: AccountLoader<'info, Airdrop>,
    pub system_program: Program<'info, System>,
}

impl CreateAirdrop<'_> {
    pub(crate) fn invoke(ctx: Context<Self>, nonce: [u8; 8], duration: u64) -> Result<()> {
        let timelock_secs = {
            let config = ctx.accounts.airdrop_config.load()?;
            config
                .get_enabled_operator(ctx.accounts.operator.key)?
                .timelock_secs
        };

        // S1.1: duration must be >= 2T.
        let two_t = timelock_secs
            .checked_mul(2)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;
        require_gte!(duration, two_t, CoreError::InvalidArgument);

        let clock = Clock::get()?;
        let expiry = clock
            .unix_timestamp
            .checked_add(i64::try_from(duration).map_err(|_| error!(CoreError::ValueOverflow))?)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        {
            let mut airdrop = ctx.accounts.airdrop.load_init()?;
            airdrop.init(
                ctx.bumps.airdrop,
                &ctx.accounts.store.key(),
                ctx.accounts.operator.key,
                nonce,
                expiry,
            )?;
        }

        emit!(AirdropCreated::new(
            ctx.accounts.store.key(),
            ctx.accounts.airdrop.key(),
            *ctx.accounts.operator.key,
            nonce,
            expiry,
        )?);

        Ok(())
    }
}

// ============================================================================
// 4. add_airdrop_target (operator) - S1.2
// ============================================================================

/// Accounts definition for [`add_airdrop_target`](crate::gmsol_gt_incentive::add_airdrop_target).
#[derive(Accounts)]
#[instruction(recipient: Pubkey)]
pub struct AddAirdropTarget<'info> {
    /// The operator who created the airdrop and pays for the new target account.
    #[account(mut)]
    pub operator: Signer<'info>,
    /// CHECK: only used to scope to a specific store.
    pub store: UncheckedAccount<'info>,
    #[account(
        constraint = airdrop_config.load()?.is_initialized() @ CoreError::AirdropConfigNotInitialized,
        constraint = airdrop_config.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [AirdropConfig::SEED, store.key().as_ref()],
        bump = airdrop_config.load()?.bump,
    )]
    pub airdrop_config: AccountLoader<'info, AirdropConfig>,
    #[account(
        mut,
        constraint = airdrop.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [
            Airdrop::SEED,
            store.key().as_ref(),
            operator.key().as_ref(),
            &airdrop.load()?.nonce,
        ],
        bump = airdrop.load()?.bump,
    )]
    pub airdrop: AccountLoader<'info, Airdrop>,
    /// The new target account. Created here. PDA derivation guarantees uniqueness per (airdrop, recipient).
    #[account(
        init,
        payer = operator,
        space = 8 + AirdropTarget::INIT_SPACE,
        seeds = [AirdropTarget::SEED, airdrop.key().as_ref(), recipient.as_ref()],
        bump,
    )]
    pub airdrop_target: AccountLoader<'info, AirdropTarget>,
    pub system_program: Program<'info, System>,
}

impl AddAirdropTarget<'_> {
    pub(crate) fn invoke(ctx: Context<Self>, recipient: Pubkey, amount: u64) -> Result<()> {
        let max_amount = {
            let config = ctx.accounts.airdrop_config.load()?;
            config
                .get_enabled_operator(ctx.accounts.operator.key)?
                .max_airdrop_amount
        };

        ctx.accounts.airdrop.load()?.validate_active()?;

        let new_total = ctx
            .accounts
            .airdrop
            .load()?
            .total_amount()
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        require_gte!(max_amount, new_total, CoreError::InvalidArgument);

        {
            let mut target = ctx.accounts.airdrop_target.load_init()?;
            target.init(
                ctx.bumps.airdrop_target,
                &ctx.accounts.airdrop.key(),
                &recipient,
                amount,
            )?;
        }

        ctx.accounts.airdrop.load_mut()?.add_target(amount)?;

        emit!(AirdropTargetAdded::new(
            ctx.accounts.airdrop.key(),
            recipient,
            amount,
        )?);

        Ok(())
    }
}

// ============================================================================
// 5. complete_airdrop (operator) - S1.3
// ============================================================================

/// Accounts definition for [`complete_airdrop`](crate::gmsol_gt_incentive::complete_airdrop).
#[derive(Accounts)]
pub struct CompleteAirdrop<'info> {
    pub operator: Signer<'info>,
    /// CHECK: only used to scope to a specific store.
    pub store: UncheckedAccount<'info>,
    #[account(
        constraint = airdrop_config.load()?.is_initialized() @ CoreError::AirdropConfigNotInitialized,
        constraint = airdrop_config.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [AirdropConfig::SEED, store.key().as_ref()],
        bump = airdrop_config.load()?.bump,
    )]
    pub airdrop_config: AccountLoader<'info, AirdropConfig>,
    #[account(
        mut,
        constraint = airdrop.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [
            Airdrop::SEED,
            store.key().as_ref(),
            operator.key().as_ref(),
            &airdrop.load()?.nonce,
        ],
        bump = airdrop.load()?.bump,
    )]
    pub airdrop: AccountLoader<'info, Airdrop>,
}

impl CompleteAirdrop<'_> {
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        let max_amount = {
            let config = ctx.accounts.airdrop_config.load()?;
            config
                .get_enabled_operator(ctx.accounts.operator.key)?
                .max_airdrop_amount
        };

        require_gte!(
            max_amount,
            ctx.accounts.airdrop.load()?.total_amount(),
            CoreError::InvalidArgument
        );

        ctx.accounts.airdrop.load_mut()?.mark_complete()?;

        let (total_amount, target_count) = {
            let airdrop = ctx.accounts.airdrop.load()?;
            (airdrop.total_amount(), airdrop.target_count())
        };
        emit!(AirdropCompleted::new(
            ctx.accounts.airdrop.key(),
            total_amount,
            target_count,
        )?);

        Ok(())
    }
}

// ============================================================================
// 6. approve_airdrop (gov, with GT_CONTROLLER role) - S1.4
// ============================================================================

/// Accounts definition for [`approve_airdrop`](crate::gmsol_gt_incentive::approve_airdrop).
#[derive(Accounts)]
pub struct ApproveAirdrop<'info> {
    /// The Gov authority. Must match `airdrop_config.gov` AND hold `GT_CONTROLLER` role
    /// in the store (the latter enforced by `access_control`).
    pub authority: Signer<'info>,
    /// CHECK: validated by `CpiAuthenticate::only` via CPI to store.
    pub store: UncheckedAccount<'info>,
    #[account(
        constraint = airdrop_config.load()?.is_initialized() @ CoreError::AirdropConfigNotInitialized,
        constraint = airdrop_config.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [AirdropConfig::SEED, store.key().as_ref()],
        bump = airdrop_config.load()?.bump,
    )]
    pub airdrop_config: AccountLoader<'info, AirdropConfig>,
    #[account(
        mut,
        constraint = airdrop.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [
            Airdrop::SEED,
            store.key().as_ref(),
            airdrop.load()?.operator.as_ref(),
            &airdrop.load()?.nonce,
        ],
        bump = airdrop.load()?.bump,
    )]
    pub airdrop: AccountLoader<'info, Airdrop>,
    pub store_program: Program<'info, GmsolStore>,
}

impl ApproveAirdrop<'_> {
    /// CHECK: only `GT_CONTROLLER` is allowed to invoke (enforced via `access_control`).
    /// Additionally enforces `caller == airdrop_config.gov`.
    pub(crate) fn invoke_unchecked(ctx: Context<Self>) -> Result<()> {
        require_keys_eq!(
            *ctx.accounts.authority.key,
            *ctx.accounts.airdrop_config.load()?.gov(),
            CoreError::PermissionDenied
        );

        let (timelock_secs, max_amount) = {
            let config = ctx.accounts.airdrop_config.load()?;
            let operator = *ctx.accounts.airdrop.load()?.operator();
            let op = config.get_enabled_operator(&operator)?;
            (op.timelock_secs, op.max_airdrop_amount)
        };

        require_gte!(
            max_amount,
            ctx.accounts.airdrop.load()?.total_amount(),
            CoreError::InvalidArgument
        );

        ctx.accounts.airdrop.load_mut()?.approve(timelock_secs)?;

        let claimable_at = ctx.accounts.airdrop.load()?.claimable_at();
        emit!(AirdropApproved::new(
            ctx.accounts.airdrop.key(),
            *ctx.accounts.authority.key,
            claimable_at,
        )?);

        Ok(())
    }
}

impl<'info> WithStore<'info> for ApproveAirdrop<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }
    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for ApproveAirdrop<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }
    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

// ============================================================================
// 7. claim_airdrop_target (recipient) - S1.5
// ============================================================================

/// Accounts definition for [`claim_airdrop_target`](crate::gmsol_gt_incentive::claim_airdrop_target).
///
/// Performs CPI to store's `mint_gt_reward`, signed by the program's
/// `gt_authority` PDA which is expected to hold the `GT_CONTROLLER` role.
#[derive(Accounts)]
pub struct ClaimAirdropTarget<'info> {
    /// The recipient who claims their GT.
    pub claimer: Signer<'info>,
    /// CHECK: validated by store program via mint CPI.
    #[account(mut)]
    pub store: UncheckedAccount<'info>,
    #[account(
        constraint = airdrop.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [
            Airdrop::SEED,
            store.key().as_ref(),
            airdrop.load()?.operator.as_ref(),
            &airdrop.load()?.nonce,
        ],
        bump = airdrop.load()?.bump,
    )]
    pub airdrop: AccountLoader<'info, Airdrop>,
    /// PDA seeds tie this target to (airdrop, claimer); a wrong claimer can't pass.
    #[account(
        mut,
        constraint = airdrop_target.load()?.airdrop == airdrop.key() @ CoreError::InvalidArgument,
        seeds = [AirdropTarget::SEED, airdrop.key().as_ref(), claimer.key().as_ref()],
        bump = airdrop_target.load()?.bump,
    )]
    pub airdrop_target: AccountLoader<'info, AirdropTarget>,
    /// PDA that holds the `GT_CONTROLLER` role and signs the mint CPI.
    /// The admin must `grant_role(this_pda, GT_CONTROLLER)` once at setup.
    /// CHECK: PDA-only, no data.
    #[account(
        seeds = [GT_AUTHORITY_SEED],
        bump,
    )]
    pub gt_authority: UncheckedAccount<'info>,
    /// The claimer's UserHeader (in store program). Validated by store via the mint CPI.
    /// CHECK: validated by store via the mint CPI.
    #[account(mut)]
    pub user: UncheckedAccount<'info>,
    /// CHECK: store program's event_authority PDA, validated by Anchor's event_cpi machinery.
    pub store_event_authority: UncheckedAccount<'info>,
    pub store_program: Program<'info, GmsolStore>,
}

impl ClaimAirdropTarget<'_> {
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        // Airdrop must be approved, claimable now, and not expired.
        ctx.accounts.airdrop.load()?.validate_claimable()?;

        let amount = {
            let target = ctx.accounts.airdrop_target.load()?;
            require_keys_eq!(
                *target.recipient(),
                *ctx.accounts.claimer.key,
                CoreError::PermissionDenied
            );
            target.amount()
        };
        require_gt!(amount, 0, CoreError::InvalidArgument);

        // Mark claimed before minting to make double-spend impossible.
        ctx.accounts.airdrop_target.load_mut()?.claim()?;

        // CPI to store::mint_gt_reward, signing as our gt_authority PDA.
        let bump = ctx.bumps.gt_authority;
        let signer_seeds: &[&[u8]] = &[GT_AUTHORITY_SEED, &[bump]];

        mint_gt_reward(
            CpiContext::new_with_signer(
                ctx.accounts.store_program.to_account_info(),
                MintGtReward {
                    authority: ctx.accounts.gt_authority.to_account_info(),
                    store: ctx.accounts.store.to_account_info(),
                    user: ctx.accounts.user.to_account_info(),
                    event_authority: ctx.accounts.store_event_authority.to_account_info(),
                    program: ctx.accounts.store_program.to_account_info(),
                },
                &[signer_seeds],
            ),
            amount,
        )?;

        emit!(AirdropClaimed::new(
            ctx.accounts.airdrop.key(),
            *ctx.accounts.claimer.key,
            amount,
        )?);

        Ok(())
    }
}

// ============================================================================
// 8. cancel_airdrop (operator) - abort before approval
// ============================================================================

/// Accounts definition for [`cancel_airdrop`](crate::gmsol_gt_incentive::cancel_airdrop).
///
/// The PDA seeds for `airdrop` include `operator.key()`, so a different
/// signer would derive a different PDA and fail the seed check before
/// the instruction body runs. No explicit `require_keys_eq!` is needed.
#[derive(Accounts)]
pub struct CancelAirdrop<'info> {
    /// The operator who originally created this airdrop.
    pub operator: Signer<'info>,
    /// CHECK: only used to scope to a specific store.
    pub store: UncheckedAccount<'info>,
    #[account(
        mut,
        constraint = airdrop.load()?.store == store.key() @ CoreError::StoreMismatched,
        seeds = [
            Airdrop::SEED,
            store.key().as_ref(),
            operator.key().as_ref(),
            &airdrop.load()?.nonce,
        ],
        bump = airdrop.load()?.bump,
    )]
    pub airdrop: AccountLoader<'info, Airdrop>,
}

impl CancelAirdrop<'_> {
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        ctx.accounts.airdrop.load_mut()?.cancel()?;

        emit!(AirdropCancelled::new(
            ctx.accounts.airdrop.key(),
            *ctx.accounts.operator.key,
        )?);

        Ok(())
    }
}
