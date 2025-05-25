use crate::{
    states::{
        Competition, LeaderEntry, Participant, EXPECTED_STORE_PROGRAM_ID, MAX_LEADERBOARD_LEN,
        PARTICIPANT_SEED,
    },
    CompetitionError,
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
        bump = participant.bump,
        has_one = competition,
        has_one = trader,
    )]
    pub participant: Account<'info, Participant>,
    /// The trader public key.
    /// CHECK: Only the address is required.
    pub trader: UncheckedAccount<'info>,
    /// The action account.
    /// CHECK: this is just a placeholder.
    pub action: UncheckedAccount<'info>,
}

impl OnCallback<'_> {
    pub(crate) fn invoke_on_created(
        ctx: Context<Self>,
        _authority_bump: u8,
        action_kind: u8,
        callback_version: u8,
        _extra_account_count: u8,
    ) -> Result<()> {
        // Only callback version `0` is supported.
        require_eq!(callback_version, 0);
        // Only setup callback for orders.
        require_eq!(
            action_kind,
            ActionKind::Order as u8,
            CompetitionError::InvalidActionKind
        );

        ctx.accounts.validate_competition()?;
        Ok(())
    }

    fn validate_competition(&self) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let comp = &self.competition;
        require!(comp.is_active, CompetitionError::CompetitionNotActive);
        require!(
            now >= comp.start_time && now <= comp.end_time,
            CompetitionError::OutsideCompetitionTime
        );
        Ok(())
    }
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
        bump = participant.bump,
        has_one = competition,
        has_one = trader,
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
        callback_version: u8,
        success: bool,
        extra_account_count: u8,
    ) -> Result<()> {
        // Validate callback parameters.
        require_eq!(callback_version, 0);
        require_eq!(
            action_kind,
            ActionKind::Order as u8,
            CompetitionError::InvalidActionKind
        );
        require_gte!(extra_account_count, 2);

        // Only process successful order actions.
        if !success {
            msg!("competition: ignore failed order");
            return Ok(());
        }

        let clock = Clock::get()?;
        let now = clock.unix_timestamp;

        let comp = &mut ctx.accounts.competition;

        if !comp.is_active {
            msg!("competition: the competition is not active");
            return Ok(());
        }
        if !(now >= comp.start_time && now <= comp.end_time) {
            msg!("competition: outside of the competition time");
            return Ok(());
        }

        // Get volume from the trade event
        let Some(trade_event) = &ctx.accounts.trade_event else {
            msg!("competition: no trade event");
            return Ok(());
        };
        let trade_event = trade_event.load()?;
        let part = &mut ctx.accounts.participant;

        // Calculate volume as the absolute difference between after and before size_in_usd
        let volume = trade_event
            .after
            .size_in_usd
            .abs_diff(trade_event.before.size_in_usd);

        part.volume = part.volume.saturating_add(volume);
        part.last_updated_at = now;

        // Check if volume exceeds threshold and extend competition time if needed
        if volume >= comp.volume_threshold {
            let old_end_time = comp.end_time;
            let proposed_end_time = old_end_time.saturating_add(comp.time_extension);
            let max_end_time = now.saturating_add(comp.max_extension);

            // Take the minimum of proposed end time and max end time
            comp.end_time = proposed_end_time.min(max_end_time).max(old_end_time);

            // Record the trigger address
            comp.extension_trigger = Some(part.trader);

            debug_assert!(comp.end_time >= old_end_time);
            let actual_extension = comp.end_time - old_end_time;
            msg!(
                "competition: extended time by {} seconds due to large trade volume={} from trader={}",
                actual_extension,
                volume,
                part.trader
            );
        }

        Self::update_leaderboard(comp, part);

        msg!(
            "competition: trader={} new_volume={} volume_delta={}",
            part.trader,
            part.volume,
            volume
        );
        Ok(())
    }

    fn update_leaderboard(comp: &mut Account<Competition>, part: &Participant) {
        let entry = if let Some(pos) = comp
            .leaderboard
            .iter()
            .position(|e| e.address == part.trader)
        {
            let mut old = comp.leaderboard.remove(pos);
            old.volume = part.volume;
            old
        } else {
            LeaderEntry {
                address: part.trader,
                volume: part.volume,
            }
        };

        let insert_pos = comp
            .leaderboard
            .iter()
            .rposition(|e| e.volume >= entry.volume)
            .map(|pos| pos + 1)
            .unwrap_or(0);

        comp.leaderboard.insert(insert_pos, entry);

        if comp.leaderboard.len() > MAX_LEADERBOARD_LEN as usize {
            comp.leaderboard.truncate(MAX_LEADERBOARD_LEN as usize);
        }
    }
}
