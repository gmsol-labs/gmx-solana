use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol_store::{
    events::TradeData,
    states::{
        glv::GlvWithdrawal,
        gt::{GtExchange, GtExchangeVault, GtVesting},
        position::PositionKind,
        user::{ReferralCode, ReferralCodeBytes, UserHeader},
        Deposit, GlvDeposit, NonceBytes, Order, Position, PriceFeed, PriceProviderKind, Seed,
        Shift, Store, Withdrawal,
    },
};
use gmsol_treasury::states::{Config, GtBank, TreasuryConfig};
use gmsol_utils::to_seed;

use crate::utils::EVENT_AUTHORITY_SEED;

/// Default store.
pub fn find_default_store() -> (Pubkey, u8) {
    find_store_address("", &gmsol_store::ID)
}

/// Find PDA for `event_authority` account.
pub fn find_event_authority_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[EVENT_AUTHORITY_SEED], program_id)
}

/// Find PDA for [`Store`] account.
pub fn find_store_address(key: &str, store_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[Store::SEED, &to_seed(key)], store_program_id)
}

/// Find PDA for the market vault.
pub fn find_market_vault_address(
    store: &Pubkey,
    token: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            gmsol_store::constants::MARKET_VAULT_SEED,
            store.as_ref(),
            token.as_ref(),
            &[],
        ],
        store_program_id,
    )
}

/// Find PDA for Market token mint account.
pub fn find_market_token_address(
    store: &Pubkey,
    index_token: &Pubkey,
    long_token: &Pubkey,
    short_token: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            gmsol_store::constants::MAREKT_TOKEN_MINT_SEED,
            store.as_ref(),
            index_token.as_ref(),
            long_token.as_ref(),
            short_token.as_ref(),
        ],
        store_program_id,
    )
}

/// Create PDA for deposit.
pub fn find_deposit_address(
    store: &Pubkey,
    user: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[Deposit::SEED, store.as_ref(), user.as_ref(), nonce],
        store_program_id,
    )
}

/// Create PDA for withdrawal.
pub fn find_withdrawal_address(
    store: &Pubkey,
    user: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[Withdrawal::SEED, store.as_ref(), user.as_ref(), nonce],
        store_program_id,
    )
}

/// Create PDA for order.
pub fn find_order_address(
    store: &Pubkey,
    user: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[Order::SEED, store.as_ref(), user.as_ref(), nonce],
        store_program_id,
    )
}

/// Create PDA for shift.
pub fn find_shift_address(
    store: &Pubkey,
    owner: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[Shift::SEED, store.as_ref(), owner.as_ref(), nonce],
        store_program_id,
    )
}

/// Create PDA for position.
pub fn find_position_address(
    store: &Pubkey,
    user: &Pubkey,
    market_token: &Pubkey,
    collateral_token: &Pubkey,
    kind: PositionKind,
    store_program_id: &Pubkey,
) -> crate::Result<(Pubkey, u8)> {
    if matches!(kind, PositionKind::Uninitialized) {
        return Err(crate::Error::invalid_argument(
            "uninitialized position kind is not allowed",
        ));
    }
    Ok(Pubkey::find_program_address(
        &[
            Position::SEED,
            store.as_ref(),
            user.as_ref(),
            market_token.as_ref(),
            collateral_token.as_ref(),
            &[kind as u8],
        ],
        store_program_id,
    ))
}

/// Find PDA for claimable account.
pub fn find_claimable_account_pda(
    store: &Pubkey,
    mint: &Pubkey,
    user: &Pubkey,
    time_key: &[u8],
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            gmsol_store::constants::CLAIMABLE_ACCOUNT_SEED,
            store.as_ref(),
            mint.as_ref(),
            user.as_ref(),
            time_key,
        ],
        store_program_id,
    )
}

/// Find PDA for trade event buffer.
pub fn find_trade_event_buffer_pda(
    store: &Pubkey,
    authority: &Pubkey,
    index: u8,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            TradeData::SEED,
            store.as_ref(),
            authority.as_ref(),
            &[index],
        ],
        store_program_id,
    )
}

/// Find PDA for user account.
pub fn find_user_pda(store: &Pubkey, owner: &Pubkey, store_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[UserHeader::SEED, store.as_ref(), owner.as_ref()],
        store_program_id,
    )
}

/// Find PDA for referral code account.
pub fn find_referral_code_pda(
    store: &Pubkey,
    code: ReferralCodeBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[ReferralCode::SEED, store.as_ref(), &code],
        store_program_id,
    )
}

/// Find the PDA for a GLV deposit account.
pub fn find_glv_deposit_pda(
    store: &Pubkey,
    owner: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GlvDeposit::SEED, store.as_ref(), owner.as_ref(), nonce],
        store_program_id,
    )
}

/// Find the PDA for a GLV withdrawal account.
pub fn find_glv_withdrawal_pda(
    store: &Pubkey,
    owner: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GlvWithdrawal::SEED, store.as_ref(), owner.as_ref(), nonce],
        store_program_id,
    )
}

/// Find the PDA for GT exchange vault account.
pub fn find_gt_exchange_vault_pda(
    store: &Pubkey,
    time_window_index: i64,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            GtExchangeVault::SEED,
            store.as_ref(),
            &time_window_index.to_be_bytes(),
        ],
        store_program_id,
    )
}

/// Find the PDA for GT exchange account.
pub fn find_gt_exchange_pda(
    vault: &Pubkey,
    owner: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GtExchange::SEED, vault.as_ref(), owner.as_ref()],
        store_program_id,
    )
}

/// Find the PDA for GT vesting account.
pub fn find_gt_vesting_pda(
    store: &Pubkey,
    owner: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GtVesting::SEED, store.as_ref(), owner.as_ref()],
        store_program_id,
    )
}

/// Fint the PDA for custom price feed account.
pub fn find_price_feed_pda(
    store: &Pubkey,
    authority: &Pubkey,
    index: u8,
    provider: PriceProviderKind,
    token: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            PriceFeed::SEED,
            store.as_ref(),
            authority.as_ref(),
            &[index],
            &[u8::from(provider)],
            token.as_ref(),
        ],
        store_program_id,
    )
}

/// Find the PDA for global treasury config.
pub fn find_config_pda(store: &Pubkey, treasury_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[Config::SEED, store.as_ref()], treasury_program_id)
}

/// Find the PDA for a treasury config.
pub fn find_treasury_config_pda(
    config: &Pubkey,
    index: u8,
    treasury_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[TreasuryConfig::SEED, config.as_ref(), &[index]],
        treasury_program_id,
    )
}

/// Find the PDA for a GT bank.
pub fn find_gt_bank_pda(
    treasury_config: &Pubkey,
    gt_exchange_vault: &Pubkey,
    treasury_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            GtBank::SEED,
            treasury_config.as_ref(),
            gt_exchange_vault.as_ref(),
        ],
        treasury_program_id,
    )
}
