use gmsol_programs::gmsol_store::types::ActionHeader;
use gmsol_utils::action::ActionCallbackKind;
use solana_sdk::pubkey::Pubkey;

/// Callback.
#[derive(Debug, Clone, Copy)]
pub struct Callback {
    /// Callback program ID.
    pub program: Pubkey,
    /// The address of config account.
    pub config: Pubkey,
    /// The address of action stats account.
    pub action_stats: Pubkey,
}

impl Callback {
    pub(crate) fn from_header(header: &ActionHeader) -> crate::Result<Option<Self>> {
        let callback = match header.callback_kind()? {
            ActionCallbackKind::Disabled => None,
            ActionCallbackKind::General => Some(Self {
                program: header.callback_program_id,
                config: header.callback_config,
                action_stats: header.callback_action_stats,
            }),
            _ => return Err(crate::Error::custom("unsupported callback kind")),
        };
        Ok(callback)
    }
}

/// Callback addresses.
#[derive(Default)]
pub(crate) struct CallbackAddresses {
    pub(crate) callback_authority: Option<Pubkey>,
    pub(crate) callback_program: Option<Pubkey>,
    pub(crate) callback_config_account: Option<Pubkey>,
    pub(crate) callback_action_stats_account: Option<Pubkey>,
}
