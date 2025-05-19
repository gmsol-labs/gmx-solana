use crate::states::{
    Competition, CompetitionError, LeaderEntry, Participant, EXPECTED_STORE_PROGRAM_ID,
    MAX_LEADERBOARD_LEN, PARTICIPANT_SEED,
};
use anchor_lang::prelude::*;
use gmsol_callback::{interface::ActionKind, CALLBACK_AUTHORITY_SEED};
use gmsol_programs::gmsol_store::accounts::TradeData;

/// Generic callback accounts.
#[derive(Accounts)]
#[instruction(authority_bump: u8)]
pub struct OnCallback<'info> {
    /// The callback‑authority PDA (must be a signer).
    #[account(
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump = authority_bump,
        seeds::program = EXPECTED_STORE_PROGRAM_ID,
    )]
    pub authority: Signer<'info>,
    /// The global competition account.
    #[account(mut)]
    pub competition: Account<'info, Competition>,
    /// The participant PDA (created on demand).
    #[account(
        mut,
        seeds = [
            PARTICIPANT_SEED,
            competition.key().as_ref(),
            trader.key().as_ref(),
        ],
        bump
    )]
    pub participant: Account<'info, Participant>,
    /// The trader public key.
    /// CHECK: Only the address is required.
    pub trader: UncheckedAccount<'info>,
    /// The action account.
    /// CHECK: this is just a placeholder.
    pub action: UncheckedAccount<'info>,
}

/// Accounts for `on_executed`.
#[derive(Accounts)]
#[instruction(authority_bump: u8)]
pub struct OnExecuted<'info> {
    /// The callback‑authority PDA (must be a signer).
    #[account(
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump = authority_bump,
        seeds::program = EXPECTED_STORE_PROGRAM_ID,
    )]
    pub authority: Signer<'info>,
    /// The global competition account.
    #[account(mut)]
    pub competition: Account<'info, Competition>,
    /// The participant PDA (created on demand).
    #[account(
        mut,
        seeds = [
            PARTICIPANT_SEED,
            competition.key().as_ref(),
            trader.key().as_ref(),
        ],
        bump
    )]
    pub participant: Account<'info, Participant>,
    /// The trader public key.
    /// CHECK: Only the address is required.
    pub trader: UncheckedAccount<'info>,
    /// The action account.
    /// CHECK: this is just a placeholder.
    pub action: UncheckedAccount<'info>,
    /// CHECK: this is just a placeholder
    pub position: UncheckedAccount<'info>,
    /// Trade event data
    pub trade_event: Option<AccountLoader<'info, TradeData>>,
}

impl OnExecuted<'_> {
    /// Core entry point called by the store program.
    pub(crate) fn invoke(
        ctx: Context<Self>,
        _authority_bump: u8,
        action_kind: u8,
        success: bool,
        _extra_account_count: u8,
    ) -> Result<()> {
        // Only process successful Order actions
        if !success || action_kind != ActionKind::Order as u8 {
            return Ok(());
        }

        let clock = Clock::get()?;
        let now = clock.unix_timestamp;

        let comp = &mut ctx.accounts.competition;

        require!(comp.is_active, CompetitionError::CompetitionNotActive);
        require!(
            now >= comp.start_time && now <= comp.end_time,
            CompetitionError::OutsideCompetitionTime
        );

        let part = &mut ctx.accounts.participant;

        // First‑time init fields.
        if part.volume == 0 {
            part.competition = comp.key();
            part.owner = ctx.accounts.trader.key();
        }

        // Get volume from the trade event
        let volume = if let Some(trade_event) = &ctx.accounts.trade_event {
            let trade_event = trade_event.load()?;

            // Calculate volume as the absolute difference between after and before size_in_usd
            let volume = trade_event
                .after
                .size_in_usd
                .abs_diff(trade_event.before.size_in_usd);

            // Convert to u64, saturating if the value is too large
            volume.min(u64::MAX as u128) as u64
        } else {
            // Skip if no trade event
            return Ok(());
        };

        part.volume = part.volume.saturating_add(volume);
        part.last_updated_at = now;

        Self::update_leaderboard(comp, part);

        msg!(
            "competition: trader={} new_volume={} volume_delta={}",
            part.owner,
            part.volume,
            volume
        );
        Ok(())
    }

    fn update_leaderboard(comp: &mut Account<Competition>, part: &Participant) {
        // Try find existing entry.
        if let Some(entry) = comp
            .leaderboard
            .iter_mut()
            .find(|e| e.address == part.owner)
        {
            entry.volume = part.volume;
        } else if comp.leaderboard.len() < MAX_LEADERBOARD_LEN.into() {
            comp.leaderboard.push(LeaderEntry {
                address: part.owner,
                volume: part.volume,
            });
        } else if let Some((idx, weakest)) = comp
            .leaderboard
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.volume)
        {
            if part.volume > weakest.volume {
                comp.leaderboard[idx] = LeaderEntry {
                    address: part.owner,
                    volume: part.volume,
                };
            }
        }

        // Re‑sort in descending order.
        comp.leaderboard.sort_by(|a, b| b.volume.cmp(&a.volume));
    }
}
