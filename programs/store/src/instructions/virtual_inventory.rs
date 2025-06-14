use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

use crate::{
    internal,
    states::{
        market::virtual_inventory::{
            VirtualInventory, VIRTUAL_INVENTORY_FOR_POSITIONS_SEED,
            VIRTUAL_INVENTORY_FOR_SWAPS_SEED,
        },
        Market, Store,
    },
    CoreError,
};

/// The accounts definitions for [`close_virtual_inventory`](crate::gmsol_store::close_virtual_inventory).
#[derive(Accounts)]
pub struct CloseVirtualInventory<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The store wallet.
    #[account(mut, seeds = [Store::WALLET_SEED, store.key().as_ref()], bump)]
    pub store_wallet: SystemAccount<'info>,
    /// The virtual inventory account to close.
    #[account(
        mut,
        close = store_wallet,
        has_one = store,
        constraint = virtual_inventory.load()?.ref_count() == 0,
    )]
    pub virtual_inventory: AccountLoader<'info, VirtualInventory>,
}

impl CloseVirtualInventory<'_> {
    /// Close an unused [`VirtualInventory`] account.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_unchecked(_ctx: Context<Self>) -> Result<()> {
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for CloseVirtualInventory<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definitions of [`disable_virtual_inventory`](crate::gmsol_store::disable_virtual_inventory).
#[derive(Accounts)]
pub struct DisableVirtualInventory<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The virtual inventory account to close.
    #[account(
        mut,
        has_one = store,
        constraint = !virtual_inventory.load()?.is_disabled() @ CoreError::PreconditionsAreNotMet,
    )]
    pub virtual_inventory: AccountLoader<'info, VirtualInventory>,
}

impl<'info> internal::Authentication<'info> for DisableVirtualInventory<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl DisableVirtualInventory<'_> {
    /// Disable the given [`VirtualInventory`] account.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_unchecked(ctx: Context<Self>) -> Result<()> {
        ctx.accounts.virtual_inventory.load_mut()?.disable()?;
        Ok(())
    }
}

/// The accounts definitions of [`leave_disabled_virtual_inventory`](crate::gmsol_store::leave_disabled_virtual_inventory).
#[derive(Accounts)]
pub struct LeaveDisabledVirtualInventory<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The virtual inventory account to join.
    #[account(
        mut,
        has_one = store,
        constraint = virtual_inventory.load()?.is_disabled() @ CoreError::PreconditionsAreNotMet,
    )]
    pub virtual_inventory: AccountLoader<'info, VirtualInventory>,
    /// The market to be added to the virtual inventory.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
}

impl<'info> internal::Authentication<'info> for LeaveDisabledVirtualInventory<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl LeaveDisabledVirtualInventory<'_> {
    /// Leave a disabled [`VirtualInventory`] account.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_unchecked(ctx: Context<Self>) -> Result<()> {
        let address = ctx.accounts.virtual_inventory.key();
        let mut market = ctx.accounts.market.load_mut()?;
        let mut virtual_inventory = ctx.accounts.virtual_inventory.load_mut()?;
        market.leave_disabled_virtual_inventory_unchecked(&address, &mut virtual_inventory)?;
        Ok(())
    }
}

/// The accounts definitions for [`create_virtual_inventory_for_swaps`](crate::gmsol_store::create_virtual_inventory_for_swaps).
#[derive(Accounts)]
#[instruction(index: u32)]
pub struct CreateVirtualInventoryForSwaps<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The virtual inventory account to create.
    #[account(
        init,
        payer = authority,
        space = 8 + VirtualInventory::INIT_SPACE,
        seeds = [VIRTUAL_INVENTORY_FOR_SWAPS_SEED, store.key().as_ref(), &index.to_le_bytes()],
        bump,
    )]
    pub virtual_inventory: AccountLoader<'info, VirtualInventory>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

impl CreateVirtualInventoryForSwaps<'_> {
    /// Create a [`VirtualInventory`] account for swaps.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_unchecked(ctx: Context<Self>, index: u32) -> Result<()> {
        ctx.accounts.virtual_inventory.load_init()?.init(
            ctx.bumps.virtual_inventory,
            index,
            ctx.accounts.store.key(),
        );
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for CreateVirtualInventoryForSwaps<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definitions for
/// [`join_virtual_inventory_for_swaps`](crate::gmsol_store::join_virtual_inventory_for_swaps)
/// and [`leave_virtual_inventory_for_swaps`](crate::gmsol_store::leave_virtual_inventory_for_swaps).
#[derive(Accounts)]
pub struct JoinOrLeaveVirtualInventoryForSwaps<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The virtual inventory account to join.
    #[account(
        mut,
        seeds = [VIRTUAL_INVENTORY_FOR_SWAPS_SEED, store.key().as_ref(), &virtual_inventory.load()?.index.to_le_bytes()],
        bump = virtual_inventory.load()?.bump,
        has_one = store,
        constraint = !virtual_inventory.load()?.is_disabled() @ CoreError::PreconditionsAreNotMet,
    )]
    pub virtual_inventory: AccountLoader<'info, VirtualInventory>,
    /// The market to be added to the virtual inventory.
    #[account(mut, has_one = store, constraint = !market.load()?.is_pure())]
    pub market: AccountLoader<'info, Market>,
}

impl JoinOrLeaveVirtualInventoryForSwaps<'_> {
    /// Add the market to the given virtual inventory.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_join_unchecked(ctx: Context<Self>) -> Result<()> {
        let address = ctx.accounts.virtual_inventory.key();
        let mut virtual_inventory = ctx.accounts.virtual_inventory.load_mut()?;
        ctx.accounts
            .market
            .load_mut()?
            .join_virtual_inventory_for_swaps_unchecked(&address, &mut virtual_inventory)?;
        Ok(())
    }

    /// Remove the market from the given virtual inventory.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_leave_unchecked(ctx: Context<Self>) -> Result<()> {
        let address = ctx.accounts.virtual_inventory.key();
        let mut market = ctx.accounts.market.load_mut()?;
        let virtual_inventory_for_swaps = market
            .virtual_inventory_for_swaps()
            .ok_or_else(|| error!(CoreError::PreconditionsAreNotMet))?;
        require_keys_eq!(*virtual_inventory_for_swaps, address);
        let mut virtual_inventory = ctx.accounts.virtual_inventory.load_mut()?;
        market.leave_virtual_inventory_for_swaps_unchecked(&mut virtual_inventory)?;
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for JoinOrLeaveVirtualInventoryForSwaps<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definitions for [`create_virtual_inventory_for_positions`](crate::gmsol_store::create_virtual_inventory_for_positions).
#[derive(Accounts)]
pub struct CreateVirtualInventoryForPositions<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// Index token address.
    /// CHECK: only the address of this account is used.
    pub index_token: UncheckedAccount<'info>,
    /// The virtual inventory account to create.
    #[account(
        init,
        payer = authority,
        space = 8 + VirtualInventory::INIT_SPACE,
        seeds = [
            VIRTUAL_INVENTORY_FOR_POSITIONS_SEED,
            store.key().as_ref(),
            index_token.key.as_ref(),
        ],
        bump,
    )]
    pub virtual_inventory: AccountLoader<'info, VirtualInventory>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

impl CreateVirtualInventoryForPositions<'_> {
    /// Create a [`VirtualInventory`] account for positions.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_unchecked(ctx: Context<Self>) -> Result<()> {
        ctx.accounts.virtual_inventory.load_init()?.init(
            ctx.bumps.virtual_inventory,
            0,
            ctx.accounts.store.key(),
        );
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for CreateVirtualInventoryForPositions<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definitions for
/// [`join_virtual_inventory_for_positions`](crate::gmsol_store::join_virtual_inventory_for_positions)
/// and [`leave_virtual_inventory_for_positions`](crate::gmsol_store::leave_virtual_inventory_for_positions).
#[derive(Accounts)]
pub struct JoinOrLeaveVirtualInventoryForPositions<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The virtual inventory account to join.
    #[account(
        mut,
        seeds = [VIRTUAL_INVENTORY_FOR_POSITIONS_SEED, store.key().as_ref(), market.load()?.meta().index_token_mint.as_ref()],
        bump = virtual_inventory.load()?.bump,
        has_one = store,
        constraint = !virtual_inventory.load()?.is_disabled() @ CoreError::PreconditionsAreNotMet,
    )]
    pub virtual_inventory: AccountLoader<'info, VirtualInventory>,
    /// The market to be added to the virtual inventory.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
}

impl JoinOrLeaveVirtualInventoryForPositions<'_> {
    /// Add the market to the given virtual inventory.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_join_unchecked(ctx: Context<Self>) -> Result<()> {
        let address = ctx.accounts.virtual_inventory.key();
        let mut virtual_inventory = ctx.accounts.virtual_inventory.load_mut()?;
        ctx.accounts
            .market
            .load_mut()?
            .join_virtual_inventory_for_positions_unchecked(&address, &mut virtual_inventory)?;
        Ok(())
    }

    /// Remove the market from the given virtual inventory.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_leave_unchecked(ctx: Context<Self>) -> Result<()> {
        let address = ctx.accounts.virtual_inventory.key();
        let mut market = ctx.accounts.market.load_mut()?;
        let virtual_inventory_for_positions = market
            .virtual_inventory_for_positions()
            .ok_or_else(|| error!(CoreError::PreconditionsAreNotMet))?;
        require_keys_eq!(*virtual_inventory_for_positions, address);
        let mut virtual_inventory = ctx.accounts.virtual_inventory.load_mut()?;
        market.leave_virtual_inventory_for_positions_unchecked(&mut virtual_inventory)?;
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for JoinOrLeaveVirtualInventoryForPositions<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
