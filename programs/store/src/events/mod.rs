/// Deposit events.
mod deposit;

/// Withdrawal events.
mod withdrawal;

/// Shift events.
mod shift;

/// GLV events.
mod glv;

/// Swap events.
mod swap;

/// Order events.
mod order;

/// Trade events.
mod trade;

/// Market events.
mod market;

/// GT events.
mod gt;

pub use deposit::*;
pub use glv::*;
pub use gt::*;
pub use market::*;
pub use order::*;
pub use shift::*;
pub use swap::*;
pub use trade::*;
pub use withdrawal::*;

use anchor_lang::prelude::*;

/// Event Emitter.
#[derive(Clone, Copy)]
pub(crate) struct EventEmitter<'a, 'info> {
    event_authority: &'a AccountInfo<'info>,
    bump: u8,
}

impl<'a, 'info> EventEmitter<'a, 'info> {
    /// Create an event emitter from event authority and bump.
    pub fn new(event_authority: &'a AccountInfo<'info>, bump: u8) -> Self {
        Self {
            event_authority,
            bump,
        }
    }
}

impl<'a, 'info> From<(&'a AccountInfo<'info>, u8)> for EventEmitter<'a, 'info> {
    fn from((event_authority, bump): (&'a AccountInfo<'info>, u8)) -> Self {
        Self::new(event_authority, bump)
    }
}

impl EventEmitter<'_, '_> {
    /// Emit event through CPI with the given space.
    pub fn emit_cpi_with_space<E>(&self, event: &E, space: usize) -> Result<()>
    where
        E: Event,
    {
        event.emit_cpi_with_space(self.event_authority.clone(), self.bump, space)
    }

    /// Emit event through CPI.
    pub fn emit_cpi<E>(&self, event: &E) -> Result<()>
    where
        E: gmsol_utils::InitSpace + Event,
    {
        self.emit_cpi_with_space(event, E::INIT_SPACE)
    }
}

/// Event.
pub trait Event: borsh::BorshSerialize + anchor_lang::Discriminator {
    /// Emit this event through CPI. This is a manual implementation of `emit_cpi!`.
    fn emit_cpi_with_space(
        &self,
        event_authority: AccountInfo,
        event_authority_bump: u8,
        space: usize,
    ) -> Result<()> {
        use anchor_lang::solana_program::instruction::Instruction;

        let disc = anchor_lang::event::EVENT_IX_TAG_LE;
        let mut ix_data = Vec::with_capacity(16 + space);
        ix_data.extend_from_slice(disc);
        ix_data.extend_from_slice(Self::DISCRIMINATOR);
        self.serialize(&mut ix_data)?;
        let ix = Instruction {
            program_id: crate::ID,
            accounts: vec![AccountMeta::new_readonly(*event_authority.key, true)],
            data: ix_data,
        };
        anchor_lang::solana_program::program::invoke_signed(
            &ix,
            &[event_authority],
            &[&[
                crate::constants::EVENT_AUTHORITY_SEED,
                &[event_authority_bump],
            ]],
        )?;
        Ok(())
    }
}
