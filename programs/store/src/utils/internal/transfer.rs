use anchor_lang::prelude::*;
use anchor_spl::token_interface::{burn, mint_to, transfer_checked, Burn, MintTo, TransferChecked};

use crate::states::Store;

pub(crate) struct TransferUtils<'a, 'info> {
    store: &'a AccountLoader<'info, Store>,
    token_program: AccountInfo<'info>,
    mint: AccountInfo<'info>,
}

impl<'a, 'info> TransferUtils<'a, 'info> {
    pub(crate) fn new(
        token_program: AccountInfo<'info>,
        store: &'a AccountLoader<'info, Store>,
        mint: AccountInfo<'info>,
    ) -> Self {
        Self {
            token_program,
            mint,
            store,
        }
    }

    pub(crate) fn mint_to(&self, to: &AccountInfo<'info>, amount: u64) -> Result<()> {
        mint_to(
            self.mint_to_ctx(to)?
                .with_signer(&[&self.store.load()?.signer_seeds()]),
            amount,
        )
    }

    pub(crate) fn burn_from(&self, from: &AccountInfo<'info>, amount: u64) -> Result<()> {
        burn(
            self.burn_ctx(from)?
                .with_signer(&[&self.store.load()?.signer_seeds()]),
            amount,
        )
    }

    pub(crate) fn transfer_out(
        &self,
        from: AccountInfo<'info>,
        to: AccountInfo<'info>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        transfer_checked(
            self.transfer_ctx(from, to)
                .with_signer(&[&self.store.load()?.signer_seeds()]),
            amount,
            decimals,
        )
    }

    fn transfer_ctx(
        &self,
        from: AccountInfo<'info>,
        to: AccountInfo<'info>,
    ) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_program.clone(),
            TransferChecked {
                from,
                to,
                authority: self.store.to_account_info(),
                mint: self.mint.to_account_info(),
            },
        )
    }

    fn mint_to_ctx(
        &self,
        receiver: &AccountInfo<'info>,
    ) -> Result<CpiContext<'_, '_, '_, 'info, MintTo<'info>>> {
        Ok(CpiContext::new(
            self.token_program.clone(),
            MintTo {
                mint: self.mint.to_account_info(),
                to: receiver.clone(),
                authority: self.store.to_account_info(),
            },
        ))
    }

    fn burn_ctx(
        &self,
        vault: &AccountInfo<'info>,
    ) -> Result<CpiContext<'_, '_, '_, 'info, Burn<'info>>> {
        Ok(CpiContext::new(
            self.token_program.clone(),
            Burn {
                mint: self.mint.to_account_info(),
                from: vault.clone(),
                authority: self.store.to_account_info(),
            },
        ))
    }
}
