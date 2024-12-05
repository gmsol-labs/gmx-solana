use anchor_lang::{prelude::*, ZeroCopy};

use crate::{
    states::{
        common::action::{Action, ActionEvent, ActionParams, Closable},
        NonceBytes,
    },
    CoreError,
};

use super::Authenticate;

/// Create Action.
pub(crate) trait Create<'info, A>: Sized + anchor_lang::Bumps {
    /// Create Params.
    type CreateParams: ActionParams;

    /// Get the action account.
    fn action(&self) -> AccountInfo<'info>;

    /// Get the payer account.
    fn payer(&self) -> AccountInfo<'info>;

    /// Get the seeds of the payer.
    fn payer_seeds(&self) -> Result<Option<Vec<Vec<u8>>>> {
        Ok(None)
    }

    /// Get the system program account.
    fn system_program(&self) -> AccountInfo<'info>;

    /// Validate.
    fn validate(&self, _params: &Self::CreateParams) -> Result<()> {
        Ok(())
    }

    /// The implementation of the creation.
    fn create_impl(
        &mut self,
        params: &Self::CreateParams,
        nonce: &NonceBytes,
        bumps: &Self::Bumps,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<()>;

    /// Create Action.
    fn create(
        ctx: &mut Context<'_, '_, 'info, 'info, Self>,
        nonce: &NonceBytes,
        params: &Self::CreateParams,
    ) -> Result<()> {
        let accounts = &mut ctx.accounts;
        accounts.validate(params)?;
        accounts.transfer_execution_lamports(params)?;
        accounts.create_impl(params, nonce, &ctx.bumps, ctx.remaining_accounts)?;
        Ok(())
    }

    /// Transfer execution lamports.
    fn transfer_execution_lamports(&self, params: &Self::CreateParams) -> Result<()> {
        use crate::ops::execution_fee::TransferExecutionFeeOperation;

        let payer_seeds = self.payer_seeds()?;
        let payer_seeds = payer_seeds
            .as_ref()
            .map(|seeds| seeds.iter().map(|seed| seed.as_slice()).collect::<Vec<_>>());

        TransferExecutionFeeOperation::builder()
            .payment(self.action())
            .payer(self.payer())
            .execution_lamports(params.execution_lamports())
            .system_program(self.system_program())
            .signer_seeds(payer_seeds.as_deref())
            .build()
            .execute()
    }
}

type ShouldContinueWhenATAsAreMissing = bool;
pub(crate) type Success = bool;

/// Close Action.
pub(crate) trait Close<'info, A>: Authenticate<'info>
where
    A: Action + ZeroCopy + Owner + Closable,
{
    /// Expected keeper role.
    fn expected_keeper_role(&self) -> &str;

    /// Rent receiver.
    fn rent_receiver(&self) -> AccountInfo<'info>;

    /// Get event authority.
    fn event_authority(&self, bumps: &Self::Bumps) -> (AccountInfo<'info>, u8);

    /// Whether to skip the completion check when the authority is keeper.
    fn skip_completion_check_for_keeper(&self) -> bool {
        false
    }

    /// Validate.
    fn validate(&self) -> Result<()> {
        Ok(())
    }

    /// Process before the close.
    fn process(&self, init_if_needed: bool) -> Result<Success>;

    /// Close Action.
    fn close(ctx: &Context<'_, '_, '_, 'info, Self>, reason: &str) -> Result<()> {
        let accounts = &ctx.accounts;
        accounts.validate()?;
        let should_continue_when_atas_are_missing = accounts.preprocess()?;
        if accounts.process(should_continue_when_atas_are_missing)? {
            {
                let action_address = accounts.action().key();
                let action = accounts.action().load()?;
                let event = action.to_closed_event(&action_address, reason)?;
                let (event_authority, event_authority_bump) = accounts.event_authority(&ctx.bumps);
                event.emit_cpi(event_authority, event_authority_bump)?;
            }
            accounts.close_action_account()?;
        } else {
            msg!("Some ATAs are not initialized, skip the close");
        }
        Ok(())
    }

    /// Action.
    fn action(&self) -> &AccountLoader<'info, A>;

    /// Preprocess.
    fn preprocess(&self) -> Result<ShouldContinueWhenATAsAreMissing> {
        if *self.authority().key == self.action().load()?.header().owner {
            Ok(true)
        } else {
            self.only_role(self.expected_keeper_role())?;
            {
                let action = self.action().load()?;
                if self.skip_completion_check_for_keeper()
                    || action.header().action_state()?.is_completed_or_cancelled()
                {
                    Ok(false)
                } else {
                    err!(CoreError::PermissionDenied)
                }
            }
        }
    }

    /// Close the action account.
    fn close_action_account(&self) -> Result<()> {
        self.action().close(self.rent_receiver())
    }
}
