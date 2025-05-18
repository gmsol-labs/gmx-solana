use anchor_lang::CheckId;

pub use crate::{
    cpi::{accounts::Callback, on_closed, on_created, on_executed},
    types::ActionKind,
    CALLBACK_AUTHORITY_SEED,
};

/// Callback interface for GMX-Solana.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallbackInterface;

impl CheckId for CallbackInterface {
    fn check_id(_id: &anchor_lang::prelude::Pubkey) -> anchor_lang::Result<()> {
        Ok(())
    }
}
