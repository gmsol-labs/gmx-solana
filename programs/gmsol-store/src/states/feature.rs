use anchor_lang::prelude::*;
use gmsol_utils::config::ConfigError;

use crate::CoreError;

pub use gmsol_utils::config::{display_feature, ActionDisabledFlag, DomainDisabledFlag};

type DisabledKey = (DomainDisabledFlag, ActionDisabledFlag);

const MAX_DISABLED_FEATURES: usize = 64;
const DISABLED: u8 = u8::MAX;

/// Disabled Features State.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct DisabledFeatures {
    map: DisabledMap,
}

impl DisabledFeatures {
    pub(crate) fn get_disabled(
        &self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
    ) -> Option<bool> {
        self.map
            .get(&(domain, action))
            .map(|value| *value == DISABLED)
    }

    pub(crate) fn set_disabled(
        &mut self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
        disabled: bool,
    ) {
        let value = if disabled { DISABLED } else { 0 };
        self.map.insert(&(domain, action), value);
    }
}

fn to_key(key: &DisabledKey) -> [u8; 2] {
    [key.0 as u8, key.1 as u8]
}

gmsol_utils::fixed_map!(
    DisabledMap,
    2,
    DisabledKey,
    to_key,
    u8,
    MAX_DISABLED_FEATURES,
    0
);

impl From<ConfigError> for CoreError {
    fn from(err: ConfigError) -> Self {
        msg!("Config error: {}", err);
        match err {
            ConfigError::UnsupportedDomain => Self::Unimplemented,
        }
    }
}
