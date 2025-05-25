use gmsol_utils::oracle::PriceProviderKind;
use solana_sdk::pubkey::Pubkey;

#[allow(unused_imports)]
use gmsol_programs::gmsol_store::accounts as store_accounts;

#[cfg(treasury)]
#[allow(unused_imports)]
use gmsol_programs::gmsol_treasury::accounts as treasury_accounts;

#[cfg(timelock)]
#[allow(unused_imports)]
use gmsol_programs::gmsol_timelock::accounts as timelock_accounts;

/// Nonce bytes.
pub type NonceBytes = [u8; 32];

/// Referral code bytes.
pub type ReferralCodeBytes = [u8; 8];

/// Seed for event authority.
pub const EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";

/// Seed for [`Store`](store_accounts::Store).
pub const STORE_SEED: &[u8] = b"data_store";

/// Seed for store wallet.
pub const STORE_WALLET_SEED: &[u8] = b"store_wallet";

/// Seed for market vault.
pub const MARKET_VAULT_SEED: &[u8] = b"market_vault";

/// Seed for market token mint.
pub const MAREKT_TOKEN_MINT_SEED: &[u8] = b"market_token_mint";

/// Seed for [`Market`](store_accounts::Market).
pub const MARKET_SEED: &[u8] = b"market";

/// Seed for [`Deposit`](store_accounts::Deposit).
pub const DEPOSIT_SEED: &[u8] = b"deposit";

/// Seed for first deposit receiver.
pub const FIRST_DEPOSIT_RECEIVER_SEED: &[u8] = b"first_deposit_receiver";

/// Seed for [`Withdrawal`](store_accounts::Withdrawal).
pub const WITHDRAWAL_SEED: &[u8] = b"withdrawal";

/// Seed for [`Shift`](store_accounts::Shift).
pub const SHIFT_SEED: &[u8] = b"shift";

/// Seed for [`Order`](store_accounts::Order).
pub const ORDER_SEED: &[u8] = b"order";

/// Seed for [`Position`](store_accounts::Position).
pub const POSITION_SEED: &[u8] = b"position";

/// Seed for claimable account.
pub const CLAIMABLE_ACCOUNT_SEED: &[u8] = b"claimable_account";

/// Seed for trade event buffer account.
pub const TRADE_DATA_SEED: &[u8] = b"trade_event_data";

/// Seed for [`User`](store_accounts::UserHeader).
pub const USER_SEED: &[u8] = b"user";

/// Seed for [`ReferralCodeV2`](store_accounts::ReferralCodeV2).
pub const REFERRAL_CODE_SEED: &[u8] = b"referral_code";

/// Seed for GLV token mint.
pub const GLV_TOKEN_SEED: &[u8] = b"glv_token";

/// Seed for [`Glv`](store_accounts::Glv).
pub const GLV_SEED: &[u8] = b"glv";

/// Seed for [`GlvDeposit`](store_accounts::GlvDeposit).
pub const GLV_DEPOSIT_SEED: &[u8] = b"glv_deposit";

/// Seed for [`GlvWithdrawal`](store_accounts::GlvWithdrawal).
pub const GLV_WITHDRAWAL_SEED: &[u8] = b"glv_withdrawal";

/// Seed for [`GtExchangeVault`](store_accounts::GtExchangeVault).
pub const GT_EXCHANGE_VAULT_SEED: &[u8] = b"gt_exchange_vault";

/// Seed for [`GtExchange`](store_accounts::GtExchange).
pub const GT_EXCHANGE_SEED: &[u8] = b"gt_exchange";

/// Seed for [`PriceFeed`](store_accounts::PriceFeed).
pub const PRICE_FEED_SEED: &[u8] = b"price_feed";

/// Seed for [`Config`](treasury_accounts::Config).
#[cfg(treasury)]
pub const TREASURY_CONFIG_SEED: &[u8] = b"config";

/// Seed for [`TreasuryVaultConfig`](treasury_accounts::TreasuryVaultConfig).
#[cfg(treasury)]
pub const TREASURY_VAULT_CONFIG_SEED: &[u8] = b"treasury_vault_config";

/// Seed for [`GtBank`](treasury_accounts::GtBank).
#[cfg(treasury)]
pub const GT_BANK_SEED: &[u8] = b"gt_bank";

/// Seed for treasury receiver.
#[cfg(treasury)]
pub const TREASURY_RECEIVER_SEED: &[u8] = b"receiver";

/// Seed for [`TimelockConfig`](timelock_accounts::TimelockConfig).
#[cfg(timelock)]
pub const TIMELOCK_CONFIG_SEED: &[u8] = b"timelock_config";

/// Seed for [`Executor`](timelock_accounts::Executor).
#[cfg(timelock)]
pub const TIMELOCK_EXECUTOR_SEED: &[u8] = b"timelock_executor";

/// Seed for the exeuctor wallet.
#[cfg(timelock)]
pub const TIMELOCK_EXECUTOR_WALLET_SEED: &[u8] = b"wallet";

/// Seed for callback authority.
pub const CALLBACK_AUTHORITY_SEED: &[u8] = b"callback";

/// Seed for competition account.
#[cfg(competition)]
pub use gmsol_programs::gmsol_competition::constants::COMPETITION_SEED;

/// Seed for participant account.
#[cfg(competition)]
pub use gmsol_programs::gmsol_competition::constants::PARTICIPANT_SEED;

fn to_seed(key: &str) -> [u8; 32] {
    use solana_sdk::hash::hash;
    hash(key.as_bytes()).to_bytes()
}

/// Find PDA for `event_authority` account.
pub fn find_event_authority_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[EVENT_AUTHORITY_SEED], program_id)
}

/// Find PDA for [`Store`](store_accounts::Store) account.
pub fn find_store_address(key: &str, store_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[STORE_SEED, &to_seed(key)], store_program_id)
}

/// Find PDA for store wallet account.
pub fn find_store_wallet_address(store: &Pubkey, store_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[STORE_WALLET_SEED, store.as_ref()], store_program_id)
}

/// Find PDA for market vault.
pub fn find_market_vault_address(
    store: &Pubkey,
    token: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[MARKET_VAULT_SEED, store.as_ref(), token.as_ref()],
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
            MAREKT_TOKEN_MINT_SEED,
            store.as_ref(),
            index_token.as_ref(),
            long_token.as_ref(),
            short_token.as_ref(),
        ],
        store_program_id,
    )
}

/// Find PDA for [`Market`](store_accounts::Market) account.
pub fn find_market_address(
    store: &Pubkey,
    token: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[MARKET_SEED, store.as_ref(), token.as_ref()],
        store_program_id,
    )
}

/// Find PDA for deposit.
pub fn find_deposit_address(
    store: &Pubkey,
    user: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[DEPOSIT_SEED, store.as_ref(), user.as_ref(), nonce],
        store_program_id,
    )
}

/// Find PDA for first deposit receiver.
pub fn find_first_deposit_receiver_address(store_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[FIRST_DEPOSIT_RECEIVER_SEED], store_program_id)
}

/// Find PDA for withdrawal.
pub fn find_withdrawal_address(
    store: &Pubkey,
    user: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[WITHDRAWAL_SEED, store.as_ref(), user.as_ref(), nonce],
        store_program_id,
    )
}

/// Find PDA for shift.
pub fn find_shift_address(
    store: &Pubkey,
    owner: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[SHIFT_SEED, store.as_ref(), owner.as_ref(), nonce],
        store_program_id,
    )
}

/// Find PDA for [`Order`](store_accounts::Order) account.
pub fn find_order_address(
    store: &Pubkey,
    owner: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[ORDER_SEED, store.as_ref(), owner.as_ref(), nonce.as_ref()],
        store_program_id,
    )
}

/// Find PDA for position.
pub fn find_position_address(
    store: &Pubkey,
    owner: &Pubkey,
    market_token: &Pubkey,
    collateral_token: &Pubkey,
    is_long: bool,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    // See the definition of `PositionKind`.
    let kind = if is_long { 1 } else { 2 };
    Pubkey::find_program_address(
        &[
            POSITION_SEED,
            store.as_ref(),
            owner.as_ref(),
            market_token.as_ref(),
            collateral_token.as_ref(),
            &[kind],
        ],
        store_program_id,
    )
}

/// Find PDA for claimable account.
pub fn find_claimable_account_address(
    store: &Pubkey,
    mint: &Pubkey,
    user: &Pubkey,
    time_key: &[u8],
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            CLAIMABLE_ACCOUNT_SEED,
            store.as_ref(),
            mint.as_ref(),
            user.as_ref(),
            time_key,
        ],
        store_program_id,
    )
}

/// Find PDA for trade event buffer.
pub fn find_trade_event_buffer_address(
    store: &Pubkey,
    authority: &Pubkey,
    index: u16,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            TRADE_DATA_SEED,
            store.as_ref(),
            authority.as_ref(),
            &index.to_le_bytes(),
        ],
        store_program_id,
    )
}

/// Find PDA for [`User`](store_accounts::UserHeader) account.
pub fn find_user_address(
    store: &Pubkey,
    owner: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[USER_SEED, store.as_ref(), owner.as_ref()],
        store_program_id,
    )
}

/// Find PDA for referral code account.
pub fn find_referral_code_address(
    store: &Pubkey,
    code: ReferralCodeBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[REFERRAL_CODE_SEED, store.as_ref(), &code],
        store_program_id,
    )
}

/// Find PDA for GLV token.
pub fn find_glv_token_address(store: &Pubkey, index: u16, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GLV_TOKEN_SEED, store.as_ref(), &index.to_le_bytes()],
        program_id,
    )
}

/// Find PDA GLV account.
pub fn find_glv_address(glv_token: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[GLV_SEED, glv_token.as_ref()], program_id)
}

/// Find PDA for a GLV deposit account.
pub fn find_glv_deposit_address(
    store: &Pubkey,
    owner: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GLV_DEPOSIT_SEED, store.as_ref(), owner.as_ref(), nonce],
        store_program_id,
    )
}

/// Find PDA for a GLV withdrawal account.
pub fn find_glv_withdrawal_address(
    store: &Pubkey,
    owner: &Pubkey,
    nonce: &NonceBytes,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GLV_WITHDRAWAL_SEED, store.as_ref(), owner.as_ref(), nonce],
        store_program_id,
    )
}

/// Find PDA for GT exchange vault account.
pub fn find_gt_exchange_vault_address(
    store: &Pubkey,
    time_window_index: i64,
    time_window: u32,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            GT_EXCHANGE_VAULT_SEED,
            store.as_ref(),
            &time_window_index.to_le_bytes(),
            &time_window.to_le_bytes(),
        ],
        store_program_id,
    )
}

/// Find PDA for GT exchange account.
pub fn find_gt_exchange_address(
    vault: &Pubkey,
    owner: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GT_EXCHANGE_SEED, vault.as_ref(), owner.as_ref()],
        store_program_id,
    )
}

/// Fint PDA for custom price feed account.
pub fn find_price_feed_address(
    store: &Pubkey,
    authority: &Pubkey,
    index: u16,
    provider: PriceProviderKind,
    token: &Pubkey,
    store_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            PRICE_FEED_SEED,
            store.as_ref(),
            authority.as_ref(),
            &index.to_le_bytes(),
            &[provider as u8],
            token.as_ref(),
        ],
        store_program_id,
    )
}

/// Find PDA for global treasury config.
#[cfg(treasury)]
pub fn find_treasury_config_address(store: &Pubkey, treasury_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[TREASURY_CONFIG_SEED, store.as_ref()], treasury_program_id)
}

/// Find PDA for treasury vault config.
#[cfg(treasury)]
pub fn find_treasury_vault_config_address(
    config: &Pubkey,
    index: u16,
    treasury_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            TREASURY_VAULT_CONFIG_SEED,
            config.as_ref(),
            &index.to_le_bytes(),
        ],
        treasury_program_id,
    )
}

/// Find PDA for GT bank.
#[cfg(treasury)]
pub fn find_gt_bank_address(
    treasury_vault_config: &Pubkey,
    gt_exchange_vault: &Pubkey,
    treasury_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            GT_BANK_SEED,
            treasury_vault_config.as_ref(),
            gt_exchange_vault.as_ref(),
        ],
        treasury_program_id,
    )
}

/// Find PDA for treasury receiver.
#[cfg(treasury)]
pub fn find_treasury_receiver_address(
    config: &Pubkey,
    treasury_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[TREASURY_RECEIVER_SEED, config.as_ref()],
        treasury_program_id,
    )
}

/// Find PDA for timelock config.
#[cfg(timelock)]
pub fn find_timelock_config_address(store: &Pubkey, timelock_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[TIMELOCK_CONFIG_SEED, store.as_ref()], timelock_program_id)
}

/// Find PDA for executor.
#[cfg(timelock)]
pub fn find_executor_address(
    store: &Pubkey,
    role: &str,
    timelock_program_id: &Pubkey,
) -> crate::Result<(Pubkey, u8)> {
    Ok(Pubkey::find_program_address(
        &[
            TIMELOCK_EXECUTOR_SEED,
            store.as_ref(),
            &crate::utils::fixed_str::fixed_str_to_bytes::<
                { gmsol_programs::constants::MAX_ROLE_NAME_LEN },
            >(role)?,
        ],
        timelock_program_id,
    ))
}

/// Find PDA for executor wallet.
#[cfg(timelock)]
pub fn find_executor_wallet_address(
    executor: &Pubkey,
    timelock_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[TIMELOCK_EXECUTOR_WALLET_SEED, executor.as_ref()],
        timelock_program_id,
    )
}

/// Find PDA for callback authority.
pub fn find_callback_authority(store_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CALLBACK_AUTHORITY_SEED], store_program_id)
}

/// Find PDA for competition account.
#[cfg(competition)]
pub fn find_competition_address(
    authority: &Pubkey,
    start_time: i64,
    competition_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            COMPETITION_SEED,
            authority.as_ref(),
            &start_time.to_le_bytes(),
        ],
        competition_program_id,
    )
}

/// Find PDA for participant account.
#[cfg(competition)]
pub fn find_participant_address(
    competition: &Pubkey,
    trader: &Pubkey,
    competition_program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[PARTICIPANT_SEED, competition.as_ref(), trader.as_ref()],
        competition_program_id,
    )
}
