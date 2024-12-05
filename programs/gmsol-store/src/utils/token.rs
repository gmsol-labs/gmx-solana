use anchor_lang::{prelude::*, solana_program::system_program};
use anchor_spl::{
    associated_token::{
        create, get_associated_token_address, get_associated_token_address_with_program_id, Create,
    },
    token_interface::{close_account, transfer_checked, CloseAccount, TransferChecked},
};
use typed_builder::TypedBuilder;

/// Check if the given address is an ATA address.
pub fn is_associated_token_account(pubkey: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> bool {
    let expected = get_associated_token_address(owner, mint);
    expected == *pubkey
}

/// Check if the given address is an ATA address.
pub fn is_associated_token_account_with_program_id(
    pubkey: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    program_id: &Pubkey,
) -> bool {
    let expected = get_associated_token_address_with_program_id(owner, mint, program_id);
    expected == *pubkey
}

/// Return whether the token account must be uninitialized.
pub fn must_be_uninitialized<'info>(account: &impl AsRef<AccountInfo<'info>>) -> bool {
    let info = account.as_ref();
    *info.owner == system_program::ID
}

/// Validate token account.
pub fn validate_token_account<'info>(
    account: &impl AsRef<AccountInfo<'info>>,
    token_program_id: &Pubkey,
) -> Result<()> {
    let info = account.as_ref();

    require!(
        !(info.owner == &system_program::ID && info.lamports() == 0),
        ErrorCode::AccountNotInitialized
    );

    require_eq!(
        info.owner,
        token_program_id,
        ErrorCode::AccountOwnedByWrongProgram,
    );

    let mut data: &[u8] = &info.try_borrow_data()?;
    anchor_spl::token_interface::TokenAccount::try_deserialize(&mut data)?;

    Ok(())
}

/// Validate associated token account.
pub fn validate_associated_token_account<'info>(
    account: &impl AsRef<AccountInfo<'info>>,
    expected_owner: &Pubkey,
    expected_mint: &Pubkey,
    token_program_id: &Pubkey,
) -> Result<()> {
    use anchor_spl::token::accessor;

    validate_token_account(account, token_program_id)?;

    let info = account.as_ref();

    let mint = accessor::mint(info)?;
    require_eq!(mint, *expected_mint, ErrorCode::ConstraintTokenMint);

    let owner = accessor::authority(info)?;
    require_eq!(owner, *expected_owner, ErrorCode::ConstraintTokenOwner);

    require!(
        is_associated_token_account_with_program_id(
            info.key,
            expected_owner,
            expected_mint,
            token_program_id
        ),
        ErrorCode::AccountNotAssociatedTokenAccount
    );

    Ok(())
}

#[derive(TypedBuilder)]
pub struct TransferAllFromEscrowToATA<'a, 'info> {
    system_program: AccountInfo<'info>,
    token_program: AccountInfo<'info>,
    associated_token_program: AccountInfo<'info>,
    payer: AccountInfo<'info>,
    owner: AccountInfo<'info>,
    mint: AccountInfo<'info>,
    decimals: u8,
    ata: AccountInfo<'info>,
    escrow: AccountInfo<'info>,
    escrow_authority: AccountInfo<'info>,
    seeds: &'a [&'a [u8]],
    init_if_needed: bool,
    #[builder(default)]
    skip_owner_check: bool,
    #[builder(default)]
    keep_escrow: bool,
    rent_receiver: AccountInfo<'info>,
}

impl<'a, 'info> TransferAllFromEscrowToATA<'a, 'info> {
    /// Transfer all tokens from the escrow account to ATA. Close the escrow account after
    /// the transfer is complete if `keep_escrow` is `false`, which is the default.
    ///
    /// Return `false` if the transfer is required but the ATA is not initilaized.
    pub(crate) fn execute(self) -> Result<bool> {
        let Self {
            system_program,
            token_program,
            associated_token_program,
            payer,
            owner,
            mint,
            decimals,
            ata,
            escrow,
            escrow_authority,
            seeds,
            init_if_needed,
            skip_owner_check,
            keep_escrow,
            rent_receiver,
        } = self;

        let amount = anchor_spl::token::accessor::amount(&escrow)?;
        if amount != 0 {
            if must_be_uninitialized(&ata) {
                if !init_if_needed {
                    return Ok(false);
                }
                create(CpiContext::new(
                    associated_token_program,
                    Create {
                        payer,
                        associated_token: ata.clone(),
                        authority: owner.clone(),
                        mint: mint.clone(),
                        system_program,
                        token_program: token_program.clone(),
                    },
                ))?;
            }

            let Ok(ata_owner) = anchor_spl::token::accessor::authority(&ata) else {
                msg!("the ATA is not a valid token account, skip the transfer");
                return Ok(false);
            };

            if ata_owner != owner.key() && !skip_owner_check {
                msg!("The ATA is not owned by the owner, skip the transfer");
                return Ok(false);
            }

            transfer_checked(
                CpiContext::new(
                    token_program.clone(),
                    TransferChecked {
                        from: escrow.to_account_info(),
                        to: ata.to_account_info(),
                        mint: mint.clone(),
                        authority: escrow_authority.clone(),
                    },
                )
                .with_signer(&[seeds]),
                amount,
                decimals,
            )?;
        }
        if !keep_escrow {
            close_account(
                CpiContext::new(
                    token_program,
                    CloseAccount {
                        account: escrow.to_account_info(),
                        destination: rent_receiver,
                        authority: escrow_authority,
                    },
                )
                .with_signer(&[seeds]),
            )?;
        }
        Ok(true)
    }
}
