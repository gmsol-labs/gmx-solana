use bytemuck::Zeroable;

use crate::gmsol_treasury::types::{TokenBalance, TokenBalancesEntry, TokenConfig, TokenMapEntry};

impl Default for TokenBalance {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Default for TokenBalancesEntry {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Default for TokenConfig {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Default for TokenMapEntry {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

#[cfg(feature = "gmsol-utils")]
mod utils {
    use crate::gmsol_treasury::{
        accounts::{GtBank, TreasuryVaultConfig},
        types::{
            TokenBalance, TokenBalances, TokenBalancesEntry, TokenConfig, TokenMap, TokenMapEntry,
        },
    };
    use anchor_lang::prelude::Pubkey;
    use gmsol_utils::{
        impl_fixed_map,
        pubkey::to_bytes,
        token_config::{TokenMapAccess, TokenRecord, TokensWithFeed},
    };

    const MAX_TOKENS: usize = 16;

    impl_fixed_map!(TokenBalances, Pubkey, to_bytes, TokenBalance, MAX_TOKENS);
    impl_fixed_map!(TokenMap, Pubkey, to_bytes, TokenConfig, MAX_TOKENS);

    impl GtBank {
        /// Get the number of tokens.
        pub fn num_tokens(&self) -> usize {
            self.balances.len()
        }

        /// Get all tokens.
        pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
            self.balances
                .entries()
                .map(|(key, _)| Pubkey::new_from_array(*key))
        }

        /// Create tokens with feed.
        pub fn to_feeds(
            &self,
            map: &impl TokenMapAccess,
            treasury_vault_config: &TreasuryVaultConfig,
        ) -> crate::Result<TokensWithFeed> {
            use std::collections::BTreeSet;

            let tokens = self
                .tokens()
                .chain(treasury_vault_config.tokens())
                .collect::<BTreeSet<_>>();
            let records = tokens
                .iter()
                .map(|token| {
                    let config = map
                        .get(token)
                        .ok_or_else(|| crate::Error::custom("unknown token"))?;
                    TokenRecord::from_config(*token, config).map_err(crate::Error::custom)
                })
                .collect::<Result<Vec<_>, _>>()?;

            TokensWithFeed::try_from_records(records).map_err(crate::Error::custom)
        }
    }

    impl TreasuryVaultConfig {
        /// Get the number of tokens.
        pub fn num_tokens(&self) -> usize {
            self.tokens.len()
        }

        /// Get all tokens.
        pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
            self.tokens
                .entries()
                .map(|(key, _)| Pubkey::new_from_array(*key))
        }
    }
}
