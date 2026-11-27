//! Owner-aware account decoding and committed CPI events for an EVM proxy deployment.

use anchor_lang::{event::EVENT_IX_TAG_LE, AccountDeserialize, AnchorDeserialize, Discriminator};
use base64::Engine;
use gmsol_programs::gmsol_evm_proxy::{accounts::Config, events};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use solana_transaction_status_client_types::EncodedTransactionWithStatusMeta;

use crate::{
    decoder::TransactionDecoder,
    value::{AnchorCPIEvents, CPIEventVisitor},
    Decode, DecodeError, Decoder, Visitor,
};

crate::impl_decode_for_cpi_event!(events::ConfigCreated);
crate::impl_decode_for_cpi_event!(events::ConfigChainAllowedChanged);
crate::impl_decode_for_cpi_event!(events::ConfigPauseChanged);
crate::impl_decode_for_cpi_event!(events::ConfigAdminProposed);
crate::impl_decode_for_cpi_event!(events::ConfigAdminAccepted);
crate::impl_decode_for_cpi_event!(events::IntentConsumed);

/// Decode Config only when its owner is the selected proxy deployment.
/// Foreign owners and unrelated account discriminators return no match.
pub fn decode_config_account(
    program_id: &Pubkey,
    owner: &Pubkey,
    data: &[u8],
) -> Result<Option<Config>, DecodeError> {
    if owner != program_id || !data.starts_with(Config::DISCRIMINATOR) {
        return Ok(None);
    }
    Config::try_deserialize(&mut &data[..])
        .map(Some)
        .map_err(DecodeError::custom)
}

/// A proxy CPI event. Unknown discriminators retain their full CPI instruction data.
/// Decoding this payload alone does not establish that a transaction committed.
#[derive(Debug)]
pub enum ProxyCPIEvent {
    /// Initial Config state.
    ConfigCreated(events::ConfigCreated),
    /// Chain policy change.
    ConfigChainAllowedChanged(events::ConfigChainAllowedChanged),
    /// Pause state change.
    ConfigPauseChanged(events::ConfigPauseChanged),
    /// Pending administrator proposal or replacement.
    ConfigAdminProposed(events::ConfigAdminProposed),
    /// Administrator acceptance.
    ConfigAdminAccepted(events::ConfigAdminAccepted),
    /// Nonce consumed by a successful intent.
    IntentConsumed(events::IntentConsumed),
    /// Unrecognized event discriminator, including the CPI header.
    Unknown(Vec<u8>),
}

impl Decode for ProxyCPIEvent {
    fn decode<D: Decoder>(decoder: D) -> Result<Self, DecodeError> {
        struct EventVisitor;
        impl Visitor for EventVisitor {
            type Value = ProxyCPIEvent;

            fn visit_bytes(self, data: &[u8]) -> Result<Self::Value, DecodeError> {
                if !data.starts_with(EVENT_IX_TAG_LE) {
                    return Err(DecodeError::custom("not an Anchor CPI event"));
                }
                let payload = &data[EVENT_IX_TAG_LE.len()..];
                if payload.len() < events::ConfigCreated::DISCRIMINATOR.len() {
                    return Err(DecodeError::custom("missing proxy event discriminator"));
                }
                macro_rules! known {
                    ($event:ty, $variant:ident) => {
                        if payload.starts_with(<$event>::DISCRIMINATOR) {
                            return CPIEventVisitor::<$event>::default()
                                .visit_bytes(data)
                                .map(ProxyCPIEvent::$variant);
                        }
                    };
                }
                known!(events::ConfigCreated, ConfigCreated);
                known!(events::ConfigChainAllowedChanged, ConfigChainAllowedChanged);
                known!(events::ConfigPauseChanged, ConfigPauseChanged);
                known!(events::ConfigAdminProposed, ConfigAdminProposed);
                known!(events::ConfigAdminAccepted, ConfigAdminAccepted);
                known!(events::IntentConsumed, IntentConsumed);
                Ok(ProxyCPIEvent::Unknown(data.to_vec()))
            }
        }
        decoder.decode_bytes(EventVisitor)
    }
}

/// Decode events from successful transaction metadata for the selected deployment.
/// The canonical event authority is derived from that deployment's program ID.
/// Events keep inner-instruction order, slot and signature. Missing/failed metadata
/// and malformed recognized events return errors instead of committed changes.
pub fn decode_committed_events(
    program_id: &Pubkey,
    slot: u64,
    signature: Signature,
    transaction: &EncodedTransactionWithStatusMeta,
) -> Result<AnchorCPIEvents<ProxyCPIEvent>, DecodeError> {
    let meta = transaction
        .meta
        .as_ref()
        .ok_or_else(|| DecodeError::custom("missing transaction metadata"))?;
    if meta.err.is_some() {
        return Err(DecodeError::custom("proxy transaction did not commit"));
    }
    let mut decoder = TransactionDecoder::new(slot, signature, transaction);
    decoder.add_cpi_event_program_id(program_id)?;
    AnchorCPIEvents::decode(&decoder)
}

/// Existing child-owned Withdraw log decoder, pending CPI conversion.
pub fn decode_withdraw_event_log(
    line: &str,
) -> Result<Option<events::Withdrawn>, crate::DecodeError> {
    let Some(encoded) = line.strip_prefix("Program data: ") else {
        return Ok(None);
    };
    let data = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(crate::DecodeError::custom)?;
    if !data.starts_with(events::Withdrawn::DISCRIMINATOR) {
        return Ok(None);
    }
    events::Withdrawn::try_from_slice(&data[events::Withdrawn::DISCRIMINATOR.len()..])
        .map(Some)
        .map_err(crate::DecodeError::custom)
}


#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::{AnchorSerialize, Event};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use gmsol_programs::gmsol_evm_proxy::{constants::CONFIG_SEED, ID};
    use solana_sdk::{
        instruction::{AccountMeta, Instruction, InstructionError},
        message::Message,
        transaction::{Transaction, TransactionError, VersionedTransaction},
    };
    use solana_transaction_status_client_types::{EncodedTransaction, TransactionBinaryEncoding};

    const SLOT: u64 = 7;

    fn authority(program: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"__event_authority"], program).0
    }

    fn pause_data() -> Vec<u8> {
        let mut data = EVENT_IX_TAG_LE.to_vec();
        data.extend(
            events::ConfigPauseChanged {
                config: Pubkey::new_unique(),
                previous_paused: true,
                paused: false,
            }
            .data(),
        );
        data
    }

    fn transaction(
        program: Pubkey,
        event_authority: Pubkey,
        data: &[u8],
    ) -> EncodedTransactionWithStatusMeta {
        let payer = Pubkey::new_unique();
        let instruction = Instruction {
            program_id: program,
            accounts: vec![AccountMeta::new_readonly(event_authority, false)],
            data: vec![],
        };
        let tx = VersionedTransaction::from(Transaction::new_unsigned(Message::new(
            &[instruction],
            Some(&payer),
        )));
        let keys = tx.message.static_account_keys();
        let program_index = keys.iter().position(|key| *key == program).unwrap();
        let authority_index = keys.iter().position(|key| *key == event_authority).unwrap();
        let meta = serde_json::from_value(serde_json::json!({
            "err": null, "status": {"Ok": null}, "fee": 0,
            "preBalances": [0,0,0], "postBalances": [0,0,0],
            "innerInstructions": [{"index": 0, "instructions": [{
                "programIdIndex": program_index, "accounts": [authority_index],
                "data": bs58::encode(data).into_string(), "stackHeight": 2
            }]}]
        }))
        .unwrap();
        EncodedTransactionWithStatusMeta {
            transaction: EncodedTransaction::Binary(
                STANDARD.encode(bincode::serialize(&tx).unwrap()),
                TransactionBinaryEncoding::Base64,
            ),
            meta: Some(meta),
            version: None,
        }
    }

    #[test]
    fn committed_wrapper_decodes_selected_proxy_event() {
        let selected = Pubkey::new_unique();
        let tx = transaction(selected, authority(&selected), &pause_data());
        let signature = Signature::new_unique();
        let events = decode_committed_events(&selected, SLOT, signature, &tx).unwrap();
        assert_eq!(events.slot(), SLOT);
        assert_eq!(events.signature(), &signature);
        assert_eq!(events.events().len(), 1);
        assert_eq!(events.events()[0].owner(), &selected);
        let ProxyCPIEvent::ConfigPauseChanged(event) = events.events()[0].data() else {
            panic!("expected pause change");
        };
        assert!(event.previous_paused);
        assert!(!event.paused);
    }

    #[test]
    fn committed_wrapper_rejects_malformed_recognized_events() {
        let selected = Pubkey::new_unique();
        let valid = pause_data();
        let truncated = valid[..valid.len() - 1].to_vec();
        let mut trailing = valid.clone();
        trailing.push(0);
        for data in [truncated, trailing] {
            let tx = transaction(selected, authority(&selected), &data);
            assert!(decode_committed_events(&selected, SLOT, Signature::default(), &tx).is_err());
        }
    }

    #[test]
    fn committed_wrapper_keeps_unknown_discriminators_distinct() {
        let selected = Pubkey::new_unique();
        let mut data = EVENT_IX_TAG_LE.to_vec();
        data.extend_from_slice(&[0xff; 8]);
        let tx = transaction(selected, authority(&selected), &data);
        let events = decode_committed_events(&selected, SLOT, Signature::default(), &tx).unwrap();
        assert!(
            matches!(events.events()[0].data(), ProxyCPIEvent::Unknown(actual) if *actual == data)
        );
    }

    #[test]
    fn committed_wrapper_filters_foreign_program_and_authority() {
        let selected = Pubkey::new_unique();
        let foreign = Pubkey::new_unique();
        for (program, signer) in [
            (foreign, authority(&selected)),
            (selected, authority(&foreign)),
        ] {
            let tx = transaction(program, signer, &pause_data());
            let events =
                decode_committed_events(&selected, SLOT, Signature::default(), &tx).unwrap();
            assert!(events.events().is_empty());
        }
    }

    #[test]
    fn committed_wrapper_rejects_failed_and_missing_metadata() {
        let selected = Pubkey::new_unique();
        let mut tx = transaction(selected, authority(&selected), &pause_data());
        let error = TransactionError::InstructionError(1, InstructionError::InvalidArgument);
        let meta = tx.meta.as_mut().unwrap();
        meta.err = Some(error.clone());
        meta.status = Err(error);
        assert!(decode_committed_events(&selected, SLOT, Signature::default(), &tx).is_err());
        tx.meta = None;
        assert!(decode_committed_events(&selected, SLOT, Signature::default(), &tx).is_err());
    }

    #[test]
    fn committed_wrapper_decodes_intent_consumed_and_rejects_malformed_payloads() {
        let selected = Pubkey::new_unique();
        let event = events::IntentConsumed {
            proxy: Pubkey::new_unique(),
            nonce: 0,
            kind: 1,
        };
        let mut valid = EVENT_IX_TAG_LE.to_vec();
        valid.extend(event.data());
        let tx = transaction(selected, authority(&selected), &valid);
        let events = decode_committed_events(&selected, SLOT, Signature::default(), &tx).unwrap();
        assert_eq!(events.events().len(), 1);
        let ProxyCPIEvent::IntentConsumed(decoded) = events.events()[0].data() else {
            panic!("expected IntentConsumed CPI event");
        };
        assert_eq!(
            (decoded.proxy, decoded.nonce, decoded.kind),
            (event.proxy, event.nonce, event.kind)
        );
        let mut trailing = valid.clone();
        trailing.push(0);
        for data in [valid[..valid.len() - 1].to_vec(), trailing] {
            let tx = transaction(selected, authority(&selected), &data);
            assert!(decode_committed_events(&selected, SLOT, Signature::default(), &tx).is_err());
        }
    }

    #[test]
    fn treasury_config_is_not_a_proxy_config() {
        use crate::{gmsol::programs::GMSOLAccountData, value::OwnedDataDecoder};
        use gmsol_programs::gmsol_treasury::{
            accounts::Config as TreasuryConfig, ID as TREASURY_ID,
        };
        let treasury: TreasuryConfig = bytemuck::Zeroable::zeroed();
        let mut data = TreasuryConfig::DISCRIMINATOR.to_vec();
        data.extend_from_slice(bytemuck::bytes_of(&treasury));
        let decoded = GMSOLAccountData::decode(OwnedDataDecoder::new(&TREASURY_ID, &data)).unwrap();
        assert!(matches!(decoded, GMSOLAccountData::UnknownOwnedData(_)));
        assert!(decode_config_account(&ID, &TREASURY_ID, &data)
            .unwrap()
            .is_none());
    }

    #[test]
    fn decodes_proxy_config_account() {
        let (_, bump) = Pubkey::find_program_address(&[CONFIG_SEED], &ID);
        let config = Config {
            store: Pubkey::new_unique(),
            genesis_hash: [7; 32],
            admin: Pubkey::new_unique(),
            next_admin: None,
            paused: true,
            allowed_chain_ids: vec![],
            bump,
        };
        let mut data = Config::DISCRIMINATOR.to_vec();
        config.serialize(&mut data).unwrap();
        let decoded = decode_config_account(&ID, &ID, &data).unwrap().unwrap();
        assert!(decode_config_account(&ID, &Pubkey::new_unique(), &data)
            .unwrap()
            .is_none());
        assert_eq!(decoded.store, config.store);
        assert_eq!(decoded.genesis_hash, config.genesis_hash);
        assert_eq!(decoded.bump, bump);
    }
}
