//! This crate defines the callback interface for the GMX-Solana protocol
//! and provide an example implementation.

/// Definition of the core structure.
pub mod states;

/// Definition of the instructions.
pub mod instructions;

/// Definitions of common types.
#[cfg(feature = "types")]
pub mod types;

/// Definition of the callback interface.
#[cfg(feature = "interface")]
pub mod interface;

use anchor_lang::prelude::*;

use instructions::*;

declare_id!("9JtQ9fBS91b2YxmHXNeGE8ipQYhLd2DRGGZSV8SPTJGw");

/// Seed for the callback authority.
#[constant]
pub const CALLBACK_AUTHORITY_SEED: &[u8] = b"callback";

#[program]
pub mod gmsol_callback {
    use super::*;

    /// Initialize the [`Config`](crate::states::Config) account.
    pub fn initialize_config(ctx: Context<InitializeConfig>) -> Result<()> {
        InitializeConfig::invoke(ctx, "callback".to_string())
    }

    /// Create [`ActionStats`](crate::states::ActionStats) account idempotently.
    pub fn create_action_stats_idempotent(
        ctx: Context<CreateActionStatsIdempotent>,
        action_kind: u8,
    ) -> Result<()> {
        CreateActionStatsIdempotent::invoke(ctx, action_kind)
    }

    /// Callback expected to be invoked when an action is created.
    pub fn on_created(
        ctx: Context<OnCallback>,
        authority_bump: u8,
        action_kind: u8,
        callback_version: u8,
        extra_account_count: u8,
    ) -> Result<()> {
        OnCallback::invoke(
            On::Created,
            ctx,
            authority_bump,
            action_kind,
            callback_version,
            true,
            extra_account_count,
        )
    }

    /// Callback expected to be invoked when an action is updated.
    pub fn on_updated(
        ctx: Context<OnCallback>,
        authority_bump: u8,
        action_kind: u8,
        callback_version: u8,
        extra_account_count: u8,
    ) -> Result<()> {
        OnCallback::invoke(
            On::Updated,
            ctx,
            authority_bump,
            action_kind,
            callback_version,
            true,
            extra_account_count,
        )
    }

    /// Callback expected to be invoked when an action is executed.
    pub fn on_executed(
        ctx: Context<OnCallback>,
        authority_bump: u8,
        action_kind: u8,
        callback_version: u8,
        success: bool,
        extra_account_count: u8,
    ) -> Result<()> {
        OnCallback::invoke(
            On::Executed,
            ctx,
            authority_bump,
            action_kind,
            callback_version,
            success,
            extra_account_count,
        )
    }

    /// Callback expected to be invoked when an action is closed.
    pub fn on_closed(
        ctx: Context<OnCallback>,
        authority_bump: u8,
        action_kind: u8,
        callback_version: u8,
        extra_account_count: u8,
    ) -> Result<()> {
        OnCallback::invoke(
            On::Closed,
            ctx,
            authority_bump,
            action_kind,
            callback_version,
            true,
            extra_account_count,
        )
    }
}
