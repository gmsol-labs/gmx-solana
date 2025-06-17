use crate::{
    states::{
        Competition, LeaderEntry, Participant, CALLER_PROGRAM_ID, MAX_LEADERBOARD_LEN,
        PARTICIPANT_SEED,
    },
    CompetitionError,
};
use anchor_lang::prelude::*;
use gmsol_callback::{interface::ActionKind, CALLBACK_AUTHORITY_SEED};
use gmsol_programs::gmsol_store::accounts::TradeData;

/// Accounts for `on_created`.
#[derive(Accounts)]
#[instruction(authority_bump: u8)]
pub struct OnCreated<'info> {
    /// The callback‑authority PDA (must be a signer).
    #[account(
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump = authority_bump,
        seeds::program = CALLER_PROGRAM_ID,
    )]
    pub authority: Signer<'info>,
    /// The global competition account.
    pub competition: Account<'info, Competition>,
    /// The participant PDA (created on demand).
    #[account(
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

impl OnCreated<'_> {
    pub(crate) fn invoke(
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
        require!(
            comp.is_ongoing(now),
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
        seeds::program = CALLER_PROGRAM_ID,
    )]
    pub authority: Signer<'info>,
    /// The global competition account.
    #[account(mut)]
    pub competition: Account<'info, Competition>,
    /// The participant PDA (created on demand).
    /// CHECK: Validation is performed only during the competition.
    #[account(
        mut,
        seeds = [
            PARTICIPANT_SEED,
            competition.key().as_ref(),
            trader.key().as_ref(),
        ],
        bump
    )]
    pub participant: UncheckedAccount<'info>,
    /// The trader public key.
    /// CHECK: Only the address is required.
    pub trader: UncheckedAccount<'info>,
    /// The action account.
    /// CHECK: this is just a placeholder.
    pub action: UncheckedAccount<'info>,
    /// CHECK: this is just a placeholder.
    pub position: UncheckedAccount<'info>,
    /// Trade event data.
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

        if !comp.is_ongoing(now) {
            msg!("competition: outside of the competition time");
            return Ok(());
        }

        // Get volume from the trade event.
        let volume = {
            let Some(trade_event) = &ctx.accounts.trade_event else {
                msg!("competition: no trade event");
                return Ok(());
            };
            let trade_event = trade_event
                .load()
                .map_err(|_| CompetitionError::InvalidTradeEvent)?;

            // Validate that the trade event belongs to the trader.
            require_keys_eq!(
                trade_event.user,
                ctx.accounts.trader.key(),
                CompetitionError::InvalidTradeEvent
            );

            // Calculate volume as the absolute difference between after and before size_in_usd.
            let volume = if comp.only_count_increase {
                // Only count volume from position increases
                trade_event
                    .after
                    .size_in_usd
                    .saturating_sub(trade_event.before.size_in_usd)
            } else {
                // Count all volume changes
                trade_event
                    .after
                    .size_in_usd
                    .abs_diff(trade_event.before.size_in_usd)
            };

            // Skip trades with zero volume.
            if volume == 0 {
                msg!("competition: skipped trade with zero volume");
                return Ok(());
            }
            volume
        };

        ctx.accounts.with_participant(|comp, part| {
            part.volume = part.volume.saturating_add(volume);

            // Determine if trade volume should be merged based on time window.
            let time_diff = now.saturating_sub(part.last_updated_at);
            part.last_updated_at = now;
            if time_diff <= comp.volume_merge_window {
                // Within the merge window, add to merged volume.
                part.merged_volume = part.merged_volume.saturating_add(volume);

                // Check if merged volume exceeds threshold.
                if part.merged_volume >= comp.volume_threshold {
                    Self::extend_competition_time(comp, part, part.merged_volume)?;
                    // Reset merged volume after triggering extension.
                    part.merged_volume = 0;
                }
            } else {
                // Outside the merge window, check single trade volume.
                if volume >= comp.volume_threshold {
                    Self::extend_competition_time(comp, part, volume)?;
                    part.merged_volume = 0;
                } else {
                    part.merged_volume = volume;
                }
            }

            Self::update_leaderboard(comp, part);

            msg!(
                "competition: trader={} new_volume={} volume_delta={} merged_volume={}",
                part.trader,
                part.volume,
                volume,
                part.merged_volume
            );

            Ok(())
        })?;

        Ok(())
    }

    fn with_participant(
        &mut self,
        f: impl FnOnce(&mut Competition, &mut Participant) -> Result<()>,
    ) -> Result<()> {
        let AccountInfo {
            key,
            lamports,
            data,
            owner,
            rent_epoch,
            is_signer,
            is_writable,
            executable,
        } = self.participant.as_ref();
        let mut lamports = lamports.borrow_mut();
        let mut data = data.borrow_mut();
        let info = AccountInfo::new(
            key,
            *is_signer,
            *is_writable,
            *lamports,
            *data,
            owner,
            *executable,
            *rent_epoch,
        );
        let mut participant = Account::<Participant>::try_from(&info)?;
        require_keys_eq!(participant.trader, self.trader.key());
        require_keys_eq!(participant.competition, self.competition.key());
        (f)(&mut self.competition, &mut participant)
    }

    /// Extend competition time if volume exceeds threshold.
    fn extend_competition_time(
        comp: &mut Competition,
        part: &Participant,
        volume: u128,
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let old_end_time = comp.end_time;
        let proposed_end_time = old_end_time.saturating_add(comp.extension_duration);
        let max_end_time = now.saturating_add(comp.extension_cap);

        // Take the minimum of proposed end time and max end time.
        comp.end_time = proposed_end_time.min(max_end_time).max(old_end_time);

        // Record the trigger address.
        comp.extension_triggerer = Some(part.trader);

        debug_assert!(comp.end_time >= old_end_time);
        let actual_extension = comp.end_time - old_end_time;
        msg!(
            "competition: extended time by {} seconds due to volume={} from trader={}",
            actual_extension,
            volume,
            part.trader
        );
        Ok(())
    }

    fn update_leaderboard(comp: &mut Competition, part: &Participant) {
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

        if insert_pos < MAX_LEADERBOARD_LEN as usize {
            comp.leaderboard.insert(insert_pos, entry);
            if comp.leaderboard.len() > MAX_LEADERBOARD_LEN as usize {
                comp.leaderboard.truncate(MAX_LEADERBOARD_LEN as usize);
            }
        }
    }
}

/// Accounts for other callbacks.
#[derive(Accounts)]
#[instruction(authority_bump: u8)]
pub struct OnCallback<'info> {
    /// The callback‑authority PDA (must be a signer).
    #[account(
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump = authority_bump,
        seeds::program = CALLER_PROGRAM_ID,
    )]
    pub authority: Signer<'info>,
    /// CHECK: No need to validate the competition account.
    pub competition: UncheckedAccount<'info>,
    /// CHECK: No need to validate the participant account.
    pub participant: UncheckedAccount<'info>,
    /// The trader public key.
    /// CHECK: Only the address is required.
    pub trader: UncheckedAccount<'info>,
    /// The action account.
    /// CHECK: this is just a placeholder.
    pub action: UncheckedAccount<'info>,
}
