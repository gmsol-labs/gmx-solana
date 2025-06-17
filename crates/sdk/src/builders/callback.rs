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
    /// The address of shared data account.
    #[builder(setter(into))]
    pub shared_data: StringPubkey,
    /// The address of partitioned data account.
    #[builder(setter(into))]
    pub partitioned_data: StringPubkey,
}

impl Callback {
    /// Create from [`ActionHeader`].
    pub fn from_header(header: &ActionHeader) -> crate::Result<Option<Self>> {
        let callback = match header.callback_kind()? {
            ActionCallbackKind::Disabled => None,
            ActionCallbackKind::General => Some(Self {
                version: header.callback_version,
                program: header.callback_program_id.into(),
                shared_data: header.callback_shared_data.into(),
                partitioned_data: header.callback_partitioned_data.into(),
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
    pub(crate) callback_shared_data_account: Option<Pubkey>,
    pub(crate) callback_partitioned_data_account: Option<Pubkey>,
}
