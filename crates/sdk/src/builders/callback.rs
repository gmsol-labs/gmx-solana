use gmsol_programs::gmsol_store::types::ActionHeader;
use gmsol_utils::action::ActionCallbackKind;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::serde::StringPubkey;

/// Callback.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct Callback {
    /// Callback version.
    pub version: u8,
    /// Callback program ID.
    #[builder(setter(into))]
    pub program: StringPubkey,
    /// The address of config account.
    #[builder(setter(into))]
    pub config: StringPubkey,
    /// The address of action stats account.
    #[builder(setter(into))]
    pub action_stats: StringPubkey,
}

impl Callback {
    /// Create from [`ActionHeader`].
    pub fn from_header(header: &ActionHeader) -> crate::Result<Option<Self>> {
        let callback = match header.callback_kind()? {
            ActionCallbackKind::Disabled => None,
            ActionCallbackKind::General => Some(Self {
                version: header.callback_version,
                program: header.callback_program_id.into(),
                config: header.callback_config.into(),
                action_stats: header.callback_action_stats.into(),
            }),
            _ => return Err(crate::Error::custom("unsupported callback kind")),
        };
        Ok(callback)
    }
}

/// Callback parameters.
#[derive(Default)]
pub(crate) struct CallbackParams {
    pub(crate) callback_version: Option<u8>,
    pub(crate) callback_authority: Option<Pubkey>,
    pub(crate) callback_program: Option<Pubkey>,
    pub(crate) callback_config_account: Option<Pubkey>,
    pub(crate) callback_action_stats_account: Option<Pubkey>,
}
