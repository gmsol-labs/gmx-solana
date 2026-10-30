//! Typed decoding for EVM proxy Config log events.

use anchor_lang::Discriminator;
use base64::Engine;
use gmsol_programs::gmsol_evm_proxy::events;

/// One decoded Config change event.
#[derive(Debug)]
pub enum ConfigEvent {
    /// Config creation.
    Created(events::ConfigCreated),
    /// Allowed-chain mutation.
    ChainAllowedChanged(events::ConfigChainAllowedChanged),
    /// Pause mutation.
    PauseChanged(events::ConfigPauseChanged),
    /// Pending-admin proposal or replacement.
    AdminProposed(events::ConfigAdminProposed),
    /// Pending-admin acceptance.
    AdminAccepted(events::ConfigAdminAccepted),
}

/// Decode a `Program data:` log produced by the EVM proxy program.
///
/// The caller must associate the log with this program's invocation and count it
/// only when the containing transaction succeeded. Other log lines return `None`.
pub fn decode_config_event_log(line: &str) -> crate::Result<Option<ConfigEvent>> {
    let Some(encoded) = line.strip_prefix("Program data: ") else {
        return Ok(None);
    };
    let data = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    macro_rules! decode {
        ($event:ty, $variant:ident) => {
            if data.starts_with(<$event>::DISCRIMINATOR) {
                return crate::utils::events::decode_anchor_event_with_options::<$event>(
                    &data, false,
                )
                .map(ConfigEvent::$variant)
                .map(Some);
            }
        };
    }
    decode!(events::ConfigCreated, Created);
    decode!(events::ConfigChainAllowedChanged, ChainAllowedChanged);
    decode!(events::ConfigPauseChanged, PauseChanged);
    decode!(events::ConfigAdminProposed, AdminProposed);
    decode!(events::ConfigAdminAccepted, AdminAccepted);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use anchor_lang::Event;
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    #[test]
    fn decodes_config_event_and_ignores_other_logs() {
        let config = Pubkey::new_unique();
        let event = events::ConfigPauseChanged {
            config,
            previous_paused: true,
            paused: false,
        };
        let log = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(event.data())
        );
        let Some(ConfigEvent::PauseChanged(decoded)) = decode_config_event_log(&log).unwrap()
        else {
            panic!("expected pause change event");
        };
        assert_eq!(decoded.config, config);
        assert!(decoded.previous_paused);
        assert!(!decoded.paused);
        assert!(decode_config_event_log("Program log: other")
            .unwrap()
            .is_none());
    }
}
