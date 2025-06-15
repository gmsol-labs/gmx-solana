use std::{num::NonZeroU64, str::FromStr};

use anchor_lang::{prelude::*, solana_program::last_restart_slot::LastRestartSlot};
use bytemuck::Zeroable;
use gmsol_utils::to_seed;

use crate::{constants, states::feature::display_feature, CoreError, CoreResult};

use super::{
    feature::{ActionDisabledFlag, DisabledFeatures, DomainDisabledFlag},
    gt::GtState,
    Amount, Factor, InitSpace, RoleKey, RoleStore, Seed,
};

pub use gmsol_utils::config::{AddressKey, AmountKey, FactorKey};

const MAX_LEN: usize = 32;

/// Data Store.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Store {
    version: u8,
    bump: [u8; 1],
    key_seed: [u8; 32],
    key: [u8; MAX_LEN],
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 6],
    role: RoleStore,
    /// Store authority.
    pub authority: Pubkey,
    /// Next authority.
    pub(crate) next_authority: Pubkey,
    /// The token map to used.
    pub token_map: Pubkey,
    /// Disabled features.
    disabled_features: DisabledFeatures,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 4],
    /// Cached last cluster restart slot.
    last_restarted_slot: u64,
    /// Treasury Config.
    treasury: Treasury,
    /// Amounts.
    pub(crate) amount: Amounts,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_2: [u8; 8],
    /// Factors.
    pub(crate) factor: Factors,
    /// Addresses.
    pub(crate) address: Addresses,
    /// GT State.
    gt: GtState,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 1024],
}

static_assertions::const_assert!(Store::INIT_SPACE + 8 <= 10240);

impl InitSpace for Store {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for Store {
    /// The value of the seed is `b"data_store"`
    const SEED: &'static [u8] = b"data_store";
}

#[cfg(feature = "display")]
impl std::fmt::Display for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Store({}): authority={} roles={} members={} token_map={} treasury={}",
            self.key()
                .map(|s| if s.is_empty() { "*default*" } else { s })
                .unwrap_or("*failed to parse*"),
            self.authority,
            self.role.num_roles(),
            self.role.num_members(),
            self.token_map()
                .map(|pubkey| pubkey.to_string())
                .unwrap_or("*unset*".to_string()),
            self.treasury,
        )
    }
}

impl Store {
    /// Maximum length of key.
    pub const MAX_LEN: usize = MAX_LEN;

    /// Wallet Seed.
    pub const WALLET_SEED: &'static [u8] = b"store_wallet";

    /// Initialize.
    pub fn init(
        &mut self,
        authority: Pubkey,
        key: &str,
        bump: u8,
        receiver: Pubkey,
        holding: Pubkey,
    ) -> Result<()> {
        self.key = crate::utils::fixed_str::fixed_str_to_bytes(key)?;
        self.key_seed = to_seed(key);
        self.bump = [bump];
        self.authority = authority;
        self.next_authority = authority;
        self.treasury.init(receiver);
        self.amount.init();
        self.factor.init();
        self.address.init(holding);

        self.update_last_restarted_slot(false)?;

        Ok(())
    }

    pub(crate) fn signer_seeds(&self) -> [&[u8]; 3] {
        [Self::SEED, &self.key_seed, &self.bump]
    }

    /// Get the role store.
    pub fn role(&self) -> &RoleStore {
        &self.role
    }

    /// Get the key of the store.
    pub fn key(&self) -> Result<&str> {
        crate::utils::fixed_str::bytes_to_fixed_str(&self.key)
    }

    /// Enable a role.
    pub fn enable_role(&mut self, role: &str) -> Result<()> {
        self.role.enable_role(role)
    }

    /// Disable a role.
    pub fn disable_role(&mut self, role: &str) -> Result<()> {
        self.role.disable_role(role)
    }

    /// Check if the roles has the given enabled role.
    /// Returns `true` only when the `role` is enabled and the `roles` has that role.
    ///
    /// # Note
    /// - If the cluster [has restarted](Self::has_restarted), this function returns `true` if and only if
    ///   the `authority` has the [`RESTART_ADMIN`](RoleKey::RESTART_ADMIN) role.
    pub fn has_role(&self, authority: &Pubkey, role: &str) -> Result<bool> {
        if self.has_restarted()? {
            if self.role.has_role(authority, RoleKey::RESTART_ADMIN)? {
                Ok(true)
            } else {
                err!(CoreError::StoreOutdated)
            }
        } else {
            self.role.has_role(authority, role)
        }
    }

    /// Grant a role.
    pub fn grant(&mut self, authority: &Pubkey, role: &str) -> Result<()> {
        self.role.grant(authority, role)
    }

    /// Revoke a role.
    pub fn revoke(&mut self, authority: &Pubkey, role: &str) -> Result<()> {
        self.role.revoke(authority, role)
    }

    /// Check if the given pubkey is the authority of the store.
    pub fn is_authority(&self, authority: &Pubkey) -> bool {
        self.authority == *authority
    }

    /// Check if the given authority has the ADMIN role.
    ///
    /// # Note
    /// - If the cluster [has restarted](Self::has_restarted), addresses with the
    ///   [`RESTART_ADMIN`](RoleKey::RESTART_ADMIN) role also have the ADMIN role.
    pub fn has_admin_role(&self, authority: &Pubkey) -> Result<bool> {
        if self.is_authority(authority) {
            Ok(true)
        } else if self.has_restarted()? {
            self.role.has_role(authority, RoleKey::RESTART_ADMIN)
        } else {
            Ok(false)
        }
    }

    pub(crate) fn set_next_authority(&mut self, next_authority: &Pubkey) -> Result<()> {
        require_keys_neq!(
            self.next_authority,
            *next_authority,
            CoreError::PreconditionsAreNotMet
        );
        self.next_authority = *next_authority;
        Ok(())
    }

    pub(crate) fn update_authority(&mut self) -> Result<Pubkey> {
        require_keys_neq!(
            self.authority,
            self.next_authority,
            CoreError::PreconditionsAreNotMet
        );
        self.authority = self.next_authority;
        Ok(self.authority)
    }

    /// Get token map address.
    pub fn token_map(&self) -> Option<&Pubkey> {
        if self.token_map == Pubkey::zeroed() {
            None
        } else {
            Some(&self.token_map)
        }
    }

    /// Get amount.
    pub fn get_amount(&self, key: &str) -> Result<&Amount> {
        let key = AmountKey::from_str(key).map_err(|_| error!(CoreError::InvalidStoreConfigKey))?;
        self.get_amount_by_key(key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Get amount by key.
    #[inline]
    pub fn get_amount_by_key(&self, key: AmountKey) -> Option<&Amount> {
        self.amount.get(&key)
    }

    /// Get amount mutably
    pub fn get_amount_mut(&mut self, key: &str) -> Result<&mut Amount> {
        let key = AmountKey::from_str(key).map_err(|_| error!(CoreError::InvalidStoreConfigKey))?;
        // Note: Changes to `claimable_time_window` are prohibited until a better
        // design of claimable account is implemented.
        require!(
            !matches!(key, AmountKey::ClaimableTimeWindow),
            CoreError::InvalidArgument,
        );
        self.amount
            .get_mut(&key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Get factor.
    pub fn get_factor(&self, key: &str) -> Result<&Factor> {
        let key = FactorKey::from_str(key).map_err(|_| error!(CoreError::InvalidStoreConfigKey))?;
        self.get_factor_by_key(key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Get factor by key.
    #[inline]
    pub fn get_factor_by_key(&self, key: FactorKey) -> Option<&Factor> {
        self.factor.get(&key)
    }

    /// Get factor mutably
    pub fn get_factor_mut(&mut self, key: &str) -> Result<&mut Factor> {
        let key = FactorKey::from_str(key).map_err(|_| error!(CoreError::InvalidStoreConfigKey))?;
        self.factor
            .get_mut(&key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Get address.
    pub fn get_address(&self, key: &str) -> Result<&Pubkey> {
        let key =
            AddressKey::from_str(key).map_err(|_| error!(CoreError::InvalidStoreConfigKey))?;
        self.get_address_by_key(key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Get address by key.
    #[inline]
    pub fn get_address_by_key(&self, key: AddressKey) -> Option<&Pubkey> {
        self.address.get(&key)
    }

    /// Get address mutably
    pub fn get_address_mut(&mut self, key: &str) -> Result<&mut Pubkey> {
        let key =
            AddressKey::from_str(key).map_err(|_| error!(CoreError::InvalidStoreConfigKey))?;
        self.address
            .get_mut(&key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Calculate the request expiration time.
    pub fn request_expiration_at(&self, start: i64) -> CoreResult<i64> {
        start
            .checked_add_unsigned(self.amount.request_expiration)
            .ok_or(CoreError::InvalidArgument)
    }

    /// Get claimable time window size.
    pub fn claimable_time_window(&self) -> Result<NonZeroU64> {
        NonZeroU64::new(self.amount.claimable_time_window)
            .ok_or_else(|| error!(CoreError::InvalidArgument))
    }

    /// Get claimable time window index for the given timestamp.
    pub fn claimable_time_window_index(&self, timestamp: i64) -> Result<i64> {
        let window: i64 = self
            .claimable_time_window()?
            .get()
            .try_into()
            .map_err(|_| error!(CoreError::InvalidArgument))?;
        Ok(timestamp / window)
    }

    /// Get claimable time key for the given timestamp.
    pub fn claimable_time_key(&self, timestamp: i64) -> Result<[u8; 8]> {
        let index = self.claimable_time_window_index(timestamp)?;
        Ok(index.to_le_bytes())
    }

    /// Get holding address.
    pub fn holding(&self) -> &Pubkey {
        &self.address.holding
    }

    /// Set the next receiver address of the treasury.
    pub(crate) fn set_next_receiver(&mut self, next_authority: &Pubkey) -> Result<()> {
        self.treasury.set_next_receiver(next_authority)
    }

    /// Update receiver address to the next receiver address.
    pub(crate) fn update_receiver(&mut self) -> Result<Pubkey> {
        self.treasury.update_receiver()?;
        Ok(self.receiver())
    }

    /// Validate whether fees can be claimed by this address.
    pub fn validate_claim_fees_address(&self, address: &Pubkey) -> Result<()> {
        require!(
            self.treasury.is_receiver(address),
            CoreError::PermissionDenied
        );
        Ok(())
    }

    /// Get the receiver address.
    pub fn receiver(&self) -> Pubkey {
        self.treasury.receiver
    }

    /// Get the next receiver address.
    pub fn next_receiver(&self) -> Pubkey {
        self.treasury.next_receiver
    }

    /// Get GT State.
    pub fn gt(&self) -> &GtState {
        &self.gt
    }

    /// Get GT State mutably.
    pub(crate) fn gt_mut(&mut self) -> &mut GtState {
        &mut self.gt
    }

    /// Get feature disabled.
    pub fn get_feature_disabled(
        &self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
    ) -> Option<bool> {
        self.disabled_features.get_disabled(domain, action)
    }

    /// Is the given feature disabled.
    pub fn is_feature_disabled(
        &self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
    ) -> bool {
        self.get_feature_disabled(domain, action).unwrap_or(false)
    }

    /// Validate whether the given features is enabled.
    pub fn validate_feature_enabled(
        &self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
    ) -> Result<()> {
        if self.is_feature_disabled(domain, action) {
            msg!("Feature `{}` is disabled", display_feature(domain, action));
            err!(CoreError::FeatureDisabled)
        } else {
            Ok(())
        }
    }

    /// Set features disabled.
    pub(crate) fn set_feature_disabled(
        &mut self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
        disabled: bool,
    ) {
        self.disabled_features
            .set_disabled(domain, action, disabled)
    }

    /// Returns whether the cluster has restarted since last update.
    pub fn has_restarted(&self) -> Result<bool> {
        Ok(self.last_restarted_slot != LastRestartSlot::get()?.last_restart_slot)
    }

    /// Validate the cluster has not restarted.
    pub fn validate_not_restarted(&self) -> Result<&Self> {
        require_eq!(
            self.last_restarted_slot,
            LastRestartSlot::get()?.last_restart_slot,
            CoreError::StoreOutdated
        );
        Ok(self)
    }

    /// Validate the cluster has not restarted for mutable reference.
    pub fn validate_not_restarted_mut(&mut self) -> Result<&mut Self> {
        self.validate_not_restarted()?;
        Ok(self)
    }

    pub(crate) fn update_last_restarted_slot(&mut self, update: bool) -> Result<u64> {
        let current = LastRestartSlot::get()?.last_restart_slot;
        if update {
            require_neq!(
                self.last_restarted_slot,
                current,
                CoreError::PreconditionsAreNotMet
            );
        }
        self.last_restarted_slot = current;
        Ok(self.last_restarted_slot)
    }

    /// Get order fee discount factor.
    pub fn order_fee_discount_factor(&self, rank: u8, is_referred: bool) -> Result<u128> {
        use gmsol_model::utils::apply_factor;

        let discount_factor_for_rank = self.gt().order_fee_discount_factor(rank)?;
        if is_referred {
            let discount_factor_for_referred = self
                .get_factor_by_key(FactorKey::OrderFeeDiscountForReferredUser)
                .ok_or_else(|| error!(CoreError::Unimplemented))?;
            let complement_discount_factor_for_referred = constants::MARKET_USD_UNIT
                .checked_sub(*discount_factor_for_referred)
                .ok_or_else(|| error!(CoreError::Internal))?;

            // 1 - (1 - A) * (1 - B) == A + B * (1 - A)
            let discount_factor = apply_factor::<_, { constants::MARKET_DECIMALS }>(
                &discount_factor_for_rank,
                &complement_discount_factor_for_referred,
            )
            .and_then(|factor| discount_factor_for_referred.checked_add(factor))
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

            debug_assert!(discount_factor <= constants::MARKET_USD_UNIT);

            Ok(discount_factor)
        } else {
            Ok(discount_factor_for_rank)
        }
    }
}

/// Store Wallet Signer.
pub(crate) struct StoreWalletSigner {
    store: Pubkey,
    bump_seed: [u8; 1],
}

impl StoreWalletSigner {
    pub(crate) fn new(store: Pubkey, bump: u8) -> Self {
        Self {
            store,
            bump_seed: [bump],
        }
    }

    pub(crate) fn signer_seeds(&self) -> [&[u8]; 3] {
        [Store::WALLET_SEED, self.store.as_ref(), &self.bump_seed]
    }
}

/// Treasury.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Treasury {
    /// Receiver.
    receiver: Pubkey,
    /// Next receiver.
    next_receiver: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 128],
}

#[cfg(feature = "display")]
impl std::fmt::Display for Treasury {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{ receiver={} }}", self.receiver,)
    }
}

impl Treasury {
    fn init(&mut self, receiver: Pubkey) {
        self.receiver = receiver;
        self.next_receiver = receiver;
    }

    fn is_receiver(&self, address: &Pubkey) -> bool {
        self.receiver == *address
    }

    fn set_next_receiver(&mut self, next_receiver: &Pubkey) -> Result<()> {
        require_keys_neq!(
            self.next_receiver,
            *next_receiver,
            CoreError::PreconditionsAreNotMet
        );
        self.next_receiver = *next_receiver;
        Ok(())
    }

    fn update_receiver(&mut self) -> Result<()> {
        require_keys_neq!(
            self.receiver,
            self.next_receiver,
            CoreError::PreconditionsAreNotMet
        );
        self.receiver = self.next_receiver;
        Ok(())
    }
}

/// Amounts.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Amounts {
    pub(crate) claimable_time_window: Amount,
    pub(crate) recent_time_window: Amount,
    pub(crate) request_expiration: Amount,
    pub(crate) oracle_max_age: Amount,
    pub(crate) oracle_max_timestamp_range: Amount,
    pub(crate) oracle_max_future_timestamp_excess: Amount,
    pub(crate) adl_prices_max_staleness: Amount,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [Amount; 126],
}

impl Amounts {
    fn init(&mut self) {
        self.claimable_time_window = constants::DEFAULT_CLAIMABLE_TIME_WINDOW;
        self.recent_time_window = constants::DEFAULT_RECENT_TIME_WINDOW;
        self.request_expiration = constants::DEFAULT_REQUEST_EXPIRATION;
        self.oracle_max_age = constants::DEFAULT_ORACLE_MAX_AGE;
        self.oracle_max_timestamp_range = constants::DEFAULT_ORACLE_MAX_TIMESTAMP_RANGE;
        self.oracle_max_future_timestamp_excess =
            constants::DEFAULT_ORACLE_MAX_FUTURE_TIMESTAMP_EXCESS;
        self.adl_prices_max_staleness = constants::DEFAULT_ADL_PRICES_MAX_STALENESS;
    }

    /// Get.
    fn get(&self, key: &AmountKey) -> Option<&Amount> {
        let value = match key {
            AmountKey::ClaimableTimeWindow => &self.claimable_time_window,
            AmountKey::RecentTimeWindow => &self.recent_time_window,
            AmountKey::RequestExpiration => &self.request_expiration,
            AmountKey::OracleMaxAge => &self.oracle_max_age,
            AmountKey::OracleMaxTimestampRange => &self.oracle_max_timestamp_range,
            AmountKey::OracleMaxFutureTimestampExcess => &self.oracle_max_future_timestamp_excess,
            AmountKey::AdlPricesMaxStaleness => &self.adl_prices_max_staleness,
            _ => return None,
        };
        Some(value)
    }

    /// Get mutably.
    fn get_mut(&mut self, key: &AmountKey) -> Option<&mut Amount> {
        let value = match key {
            AmountKey::ClaimableTimeWindow => &mut self.claimable_time_window,
            AmountKey::RecentTimeWindow => &mut self.recent_time_window,
            AmountKey::RequestExpiration => &mut self.request_expiration,
            AmountKey::OracleMaxAge => &mut self.oracle_max_age,
            AmountKey::OracleMaxTimestampRange => &mut self.oracle_max_timestamp_range,
            AmountKey::OracleMaxFutureTimestampExcess => {
                &mut self.oracle_max_future_timestamp_excess
            }
            AmountKey::AdlPricesMaxStaleness => &mut self.adl_prices_max_staleness,
            _ => return None,
        };
        Some(value)
    }
}

/// Factors.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Factors {
    pub(crate) oracle_ref_price_deviation: Factor,
    pub(crate) order_fee_discount_for_referred_user: Factor,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [Factor; 64],
}

impl Factors {
    fn init(&mut self) {
        self.oracle_ref_price_deviation = constants::DEFAULT_ORACLE_REF_PRICE_DEVIATION;
    }

    /// Get.
    fn get(&self, key: &FactorKey) -> Option<&Factor> {
        let value = match key {
            FactorKey::OracleRefPriceDeviation => &self.oracle_ref_price_deviation,
            FactorKey::OrderFeeDiscountForReferredUser => {
                &self.order_fee_discount_for_referred_user
            }
            _ => return None,
        };
        Some(value)
    }

    /// Get mutably.
    fn get_mut(&mut self, key: &FactorKey) -> Option<&mut Factor> {
        let value = match key {
            FactorKey::OracleRefPriceDeviation => &mut self.oracle_ref_price_deviation,
            FactorKey::OrderFeeDiscountForReferredUser => {
                &mut self.order_fee_discount_for_referred_user
            }
            _ => return None,
        };
        Some(value)
    }
}

/// Addresses.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Addresses {
    pub(crate) holding: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [Pubkey; 30],
}

impl Addresses {
    fn init(&mut self, holding: Pubkey) {
        self.holding = holding;
    }

    /// Get.
    fn get(&self, key: &AddressKey) -> Option<&Pubkey> {
        let value = match key {
            AddressKey::Holding => &self.holding,
            _ => return None,
        };
        Some(value)
    }

    /// Get mutably.
    fn get_mut(&mut self, key: &AddressKey) -> Option<&mut Pubkey> {
        let value = match key {
            AddressKey::Holding => &mut self.holding,
            _ => return None,
        };
        Some(value)
    }
}
