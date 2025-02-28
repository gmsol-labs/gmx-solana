use anchor_lang::prelude::*;

use crate::{
    internal,
    states::{user::ReferralCodeBytes, Store},
};

/// Referral Code.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct ReferralCode {
    /// Bump.
    pub bump: u8,
    /// Code bytes.
    pub code: ReferralCodeBytes,
    /// Store.
    pub store: Pubkey,
    /// Owner.
    pub owner: Pubkey,
}

/// The accounts definitions for [`migrate_referral_code`](crate::gmsol_store::migrate_referral_code) instruction.
#[derive(Accounts)]
pub struct MigrateReferralCode<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// System program.
    pub system: Program<'info, System>,
}

impl<'info> internal::Authentication<'info> for MigrateReferralCode<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

#[cfg(feature = "migration")]
pub(crate) use migration::unchecked_migrate_referral_code;

#[cfg(feature = "migration")]
mod migration {
    use anchor_lang::system_program;
    use gmsol_utils::InitSpace;

    use crate::{
        states::{user::ReferralCodeV2, Seed},
        CoreError,
    };

    use super::*;

    /// Migrate referral code.
    /// # CHECK
    /// Only MIGRATION_KEEPER is allowed to invoke.
    pub(crate) fn unchecked_migrate_referral_code<'info>(
        ctx: Context<'_, '_, 'info, 'info, MigrateReferralCode<'info>>,
    ) -> Result<()> {
        let code = &ctx.remaining_accounts[0];
        let data = ctx.accounts.validate_and_clone(code)?;
        ctx.accounts.initialize_code_v2_unchecked(&data, code)?;
        Ok(())
    }

    impl<'info> MigrateReferralCode<'info> {
        fn validate_and_clone(&self, code: &'info AccountInfo<'info>) -> Result<ReferralCode> {
            let loader = AccountLoader::<ReferralCode>::try_from(code)?;
            let pubkey = loader.key();
            let store = self.store.key();
            let code = loader.load()?;

            // Store validation.
            require_keys_eq!(code.store, store, CoreError::StoreMismatched);

            // Seeds validation.
            let expected_pubkey = Pubkey::create_program_address(
                &[
                    ReferralCodeV2::SEED,
                    store.as_ref(),
                    &code.code,
                    &[code.bump],
                ],
                &crate::ID,
            )
            .map_err(|_| error!(CoreError::InvalidArgument))?;
            require_keys_eq!(expected_pubkey, pubkey, ErrorCode::ConstraintSeeds);

            Ok(*code)
        }

        fn uninitialize_code_and_realloc(&self, account: &AccountInfo<'info>) -> Result<()> {
            let rent = Rent::get()?;

            account.try_borrow_mut_data()?.fill(0);

            let space = 8 + ReferralCodeV2::INIT_SPACE;
            let required_lamports = rent
                .minimum_balance(space)
                .max(1)
                .saturating_sub(account.lamports());

            if required_lamports > 0 {
                system_program::transfer(
                    CpiContext::new(
                        self.system.to_account_info(),
                        system_program::Transfer {
                            from: self.authority.to_account_info(),
                            to: account.clone(),
                        },
                    ),
                    required_lamports,
                )?;
            }

            account.realloc(space, false)?;
            Ok(())
        }

        /// # CHECK
        /// - `code` must be a valid [`ReferralCode`] account with `data` as its data.
        fn initialize_code_v2_unchecked(
            &self,
            data: &ReferralCode,
            code: &'info AccountInfo<'info>,
        ) -> Result<()> {
            self.uninitialize_code_and_realloc(code)?;
            let referral_code_v2 =
                AccountLoader::<ReferralCodeV2>::try_from_unchecked(&crate::ID, code)?;
            referral_code_v2
                .load_init()?
                .init(data.bump, data.code, &data.store, &data.owner);
            referral_code_v2.exit(&crate::ID)?;
            Ok(())
        }
    }
}
