use super::*;
use crate::TransactionAccess;
use serde_json::{json, Value};
use solana_sdk::{
    hash::Hash,
    message::{v0, Message, MessageHeader, VersionedMessage},
};

fn key(byte: u8) -> Pubkey {
    Pubkey::new_from_array([byte; 32])
}

fn reference_cases() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/v1-reference.json")).unwrap()
}

fn v1_bytes(name: &str) -> Vec<u8> {
    let case = reference_cases()
        .into_iter()
        .find(|case| case["name"] == name)
        .unwrap();
    STANDARD.decode(case["wire"].as_str().unwrap()).unwrap()
}

fn old_bytes(v0: bool) -> Vec<u8> {
    let header = MessageHeader {
        num_required_signatures: 3,
        num_readonly_signed_accounts: 1,
        num_readonly_unsigned_accounts: 1,
    };
    let instructions = vec![CompiledInstruction {
        program_id_index: if v0 { 3 } else { 5 },
        accounts: if v0 { vec![4, 5] } else { vec![3, 4] },
        data: vec![1, 2, 3],
    }];
    let message = if v0 {
        VersionedMessage::V0(v0::Message {
            header,
            account_keys: vec![key(1), key(2), key(3), key(6)],
            recent_blockhash: Hash::new_from_array([8; 32]),
            instructions,
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: key(9),
                writable_indexes: vec![7],
                readonly_indexes: vec![8],
            }],
        })
    } else {
        VersionedMessage::Legacy(Message {
            header,
            account_keys: (1..=6).map(key).collect(),
            recent_blockhash: Hash::new_from_array([8; 32]),
            instructions,
        })
    };
    bincode::serialize(&VersionedTransaction {
        signatures: (10..=12).map(|byte| Signature::from([byte; 64])).collect(),
        message,
    })
    .unwrap()
}

fn rpc(bytes: &[u8], v0: bool) -> EncodedTransactionWithStatusMeta {
    EncodedTransactionWithStatusMeta {
        transaction: EncodedTransaction::Binary(
            STANDARD.encode(bytes),
            TransactionBinaryEncoding::Base64,
        ),
        meta: Some(
            serde_json::from_value(json!({
                "err": null, "status": {"Ok": null}, "fee": 5000,
                "preBalances": vec![0; 6], "postBalances": vec![0; 6],
                "innerInstructions": [{"index": 0, "instructions": [{
                    "programIdIndex": if v0 { 3 } else { 5 },
                    "accounts": [if v0 { 5 } else { 4 }],
                    "data": bs58::encode([EVENT_IX_TAG_LE, &[11, 12, 13]].concat()).into_string()
                }]}],
                "loadedAddresses": {
                    "writable": if v0 { vec![key(4).to_string()] } else { vec![] },
                    "readonly": if v0 { vec![key(5).to_string()] } else { vec![] }
                }
            }))
            .unwrap(),
        ),
        // Decoder must identify the actual wire version, not trust this label.
        version: None,
    }
}

#[test]
fn all_official_v1_fields_match() {
    for case in reference_cases() {
        let bytes = STANDARD.decode(case["wire"].as_str().unwrap()).unwrap();
        let tx = rpc(&bytes, false);
        let decoder = TransactionDecoder::new(42, Signature::default(), &tx);
        let decoded = decoder.decoded_transaction_any_version().unwrap();
        let SupportedDecodedTransaction::V1(v1) = &decoded else {
            panic!("expected v1")
        };
        let message = &v1.transaction.message;
        let config = message.config;
        let actual = json!({
            "config": {
                "priority_fee": config.priority_fee,
                "compute_unit_limit": config.compute_unit_limit,
                "loaded_accounts_data_size_limit": config.loaded_accounts_data_size_limit,
                "heap_size": config.heap_size,
            },
            "header": [message.header.num_required_signatures,
                message.header.num_readonly_signed_accounts,
                message.header.num_readonly_unsigned_accounts],
            "lifetime": message.lifetime_specifier.to_string(),
            "accounts": message.account_keys.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "signatures": v1.transaction.signatures.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "instructions": message.instructions.iter().map(|ix| json!({
                "program": ix.program_id_index, "accounts": ix.accounts,
                "data": STANDARD.encode(&ix.data),
            })).collect::<Vec<_>>(),
        });
        let mut expected = case.clone();
        let fields = expected.as_object_mut().unwrap();
        fields.remove("name");
        fields.remove("wire");
        assert_eq!(actual, expected, "reference case {}", case["name"]);
        assert_eq!(decoded.slot().unwrap(), 42);
        assert_eq!(decoded.index().unwrap(), None);
        assert_eq!(decoded.signature().unwrap(), &Signature::default());
        assert_eq!(decoded.num_address_table_lookups(), 0);
        assert!(decoded.address_table_lookup(0).is_none());
        assert_eq!(decoded.num_instructions(), message.instructions.len());
        assert_eq!(decoded.instruction(0), message.instructions.first());
        assert_eq!(
            decoded.message_signature(0),
            v1.transaction.signatures.first()
        );
        assert!(std::ptr::eq(
            decoded.transaction_status_meta().unwrap(),
            tx.meta.as_ref().unwrap()
        ));
        // The original API retains its old return type and rejects v1.
        assert!(decoder.decoded_transaction().is_err());
    }
}

#[test]
fn mixed_versions_keep_old_api_accounts_and_cpi_events() {
    struct AccountCount;
    impl Visitor for AccountCount {
        type Value = usize;

        fn visit_transaction(self, tx: impl TransactionAccess) -> Result<usize, DecodeError> {
            Ok(tx.num_accounts())
        }
    }
    for (bytes, v0) in [
        (old_bytes(false), false),
        (v1_bytes("large"), false),
        (old_bytes(true), true),
        (old_bytes(false), false),
    ] {
        let tx = rpc(&bytes, v0);
        let mut decoder = TransactionDecoder::new(42, Signature::default(), &tx);
        decoder
            .add_cpi_event_authority_and_program_id(key(5), key(6))
            .unwrap();
        let decoded = decoder.decoded_transaction_any_version().unwrap();
        assert_eq!(decoder.decode_transaction(AccountCount).unwrap(), 6);
        assert_eq!(decoded.num_accounts(), 6);
        assert_eq!(decoded.num_signers(true).unwrap(), 2);
        assert_eq!(decoded.num_signers(false).unwrap(), 1);
        let keys = if v0 {
            [1, 2, 3, 6, 4, 5]
        } else {
            [1, 2, 3, 4, 5, 6]
        };
        for (idx, byte) in keys.into_iter().enumerate() {
            let account = decoded.account_meta(idx).unwrap().unwrap();
            assert_eq!(account.pubkey, key(byte));
            assert_eq!(account.is_signer, byte <= 3);
            assert_eq!(
                account.is_writable,
                matches!(byte, 1 | 2 | 4) || (!v0 && byte == 5)
            );
        }
        assert!(decoded.account_meta(6).unwrap().is_none());
        if let SupportedDecodedTransaction::LegacyOrV0(_) = &decoded {
            // Compile regression: original fields remain constructible, typed and mutable.
            let old: DecodedTransaction<'_> = decoder.decoded_transaction().unwrap();
            let wire: VersionedTransaction = old.transaction;
            let _: &VersionedMessage = &wire.message;
            let mut restored = DecodedTransaction {
                transaction: wire,
                ..old
            };
            restored.transaction = restored.transaction.clone();
            for idx in 0..6 {
                assert_eq!(
                    restored.account_meta(idx).unwrap(),
                    decoded.account_meta(idx).unwrap()
                );
            }
            assert_eq!(restored.instruction(0), decoded.instruction(0));
            assert_eq!(restored.message_signature(0), decoded.message_signature(0));
        }
        assert_eq!(decoded.num_address_table_lookups(), usize::from(v0));
        if v0 {
            let lookup = decoded.address_table_lookup(0).unwrap();
            assert_eq!(lookup.account_key, key(9));
            assert_eq!(lookup.writable_indexes, vec![7]);
            assert_eq!(lookup.readonly_indexes, vec![8]);
        }
        let events = decoder.extract_cpi_events().unwrap();
        assert_eq!(events.events.len(), 1);
        assert_eq!(events.events[0].program_id, key(6));
        assert_eq!(
            events.events[0].data,
            [EVENT_IX_TAG_LE, &[11, 12, 13]].concat()
        );
        decoder.set_cpi_event_filter(CPIEventFilter::default());
        assert!(decoder.extract_cpi_events().unwrap().events.is_empty());
    }
}

#[test]
fn binary_encodings_and_meta_requirement_are_preserved() {
    for (bytes, v0) in [
        (old_bytes(false), false),
        (old_bytes(true), true),
        (v1_bytes("config-15"), false),
    ] {
        for encoding in [
            EncodedTransaction::LegacyBinary(bs58::encode(&bytes).into_string()),
            EncodedTransaction::Binary(
                bs58::encode(&bytes).into_string(),
                TransactionBinaryEncoding::Base58,
            ),
            EncodedTransaction::Binary(STANDARD.encode(&bytes), TransactionBinaryEncoding::Base64),
        ] {
            let mut tx = rpc(&bytes, v0);
            tx.transaction = encoding;
            assert!(decodes(&tx));
            tx.meta = None;
            assert!(!decodes(&tx));
        }
    }
    for encoding in [
        EncodedTransaction::LegacyBinary("!".into()),
        EncodedTransaction::Binary("!".into(), TransactionBinaryEncoding::Base64),
    ] {
        let mut tx = rpc(&[], false);
        tx.transaction = encoding;
        assert!(!decodes(&tx));
    }
}

fn decodes(tx: &EncodedTransactionWithStatusMeta) -> bool {
    TransactionDecoder::new(0, Signature::default(), tx)
        .decoded_transaction_any_version()
        .is_ok()
}

fn rejects(bytes: &[u8]) {
    let tx = rpc(bytes, false);
    assert!(!decodes(&tx));
}

#[test]
fn rejects_truncation_unknown_versions_and_malformed_v1() {
    let bytes = v1_bytes("config-15");
    for end in 0..bytes.len() {
        rejects(&bytes[..end]);
    }
    // Header counts, unsupported prefix, malformed config masks and invalid indices.
    // Six addresses end at byte 234; full config ends at 254; two ix headers at 262.
    for (offset, value) in [
        (0, 0x82),
        (1, 0),
        (1, 13),
        (2, 3),
        (3, 64),
        (4, 0x1e),
        (4, 0x1d),
        (5, 1),
        (40, 65),
        (41, 65),
        (254, 0),
        (254, 6),
        (262, 6),
    ] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        rejects(&bad);
    }
    let mut duplicate = bytes.clone();
    duplicate[42..74].fill(2);
    rejects(&duplicate);
    for heap in [0u32, 32767, 32769, 263168] {
        let mut bad = bytes.clone();
        bad[250..254].copy_from_slice(&heap.to_le_bytes());
        rejects(&bad);
    }
    let mut bad = bytes.clone();
    bad[256..258].copy_from_slice(&u16::MAX.to_le_bytes());
    rejects(&bad);
    let mut trailing = bytes;
    trailing.push(0);
    rejects(&trailing);
    let mut too_large = v1_bytes("max-size");
    assert_eq!(too_large.len(), 4096);
    // Add one byte to the first ix payload and its length: valid structure, over size limit.
    let len = u16::from_le_bytes([too_large[236], too_large[237]]);
    too_large[236..238].copy_from_slice(&(len + 1).to_le_bytes());
    too_large.insert(246, 0x42);
    rejects(&too_large);
}

#[test]
fn rejects_v1_loaded_addresses_and_requires_meta_for_events() {
    let mut tx = rpc(&v1_bytes("config-0"), true);
    assert!(!decodes(&tx));
    tx.meta.as_mut().unwrap().loaded_addresses = OptionSerializer::Skip;
    tx.meta.as_mut().unwrap().inner_instructions = OptionSerializer::Skip;
    let decoder = TransactionDecoder::new(0, Signature::default(), &tx);
    assert!(decoder.decoded_transaction_any_version().is_ok());
    assert!(decoder.extract_cpi_events().is_err());
}

#[cfg(feature = "gmsol-programs")]
#[test]
fn all_versions_decode_typed_order_events_through_existing_visitor() {
    use crate::gmsol::programs::GMSOLCPIEvent;
    use anchor_lang::{AnchorSerialize, Discriminator};
    use gmsol_programs::gmsol_store::{
        events::OrderRemoved,
        types::{ActionState, OrderKind},
    };

    let event = OrderRemoved {
        id: 123,
        ts: 456,
        slot: 42,
        store: key(10),
        order: key(11),
        kind: OrderKind::MarketIncrease,
        market_token: key(12),
        owner: key(13),
        state: ActionState::Completed,
        reason: "executed".to_owned(),
    };
    let payload = [
        EVENT_IX_TAG_LE,
        OrderRemoved::DISCRIMINATOR,
        &event.try_to_vec().unwrap(),
    ]
    .concat();
    for (bytes, v0) in [
        (old_bytes(false), false),
        (old_bytes(true), true),
        (v1_bytes("large"), false),
    ] {
        let mut tx = rpc(&bytes, v0);
        let OptionSerializer::Some(inner) = &mut tx.meta.as_mut().unwrap().inner_instructions
        else {
            unreachable!()
        };
        let UiInstruction::Compiled(ix) = &mut inner[0].instructions[0] else {
            unreachable!()
        };
        ix.data = bs58::encode(&payload).into_string();
        let mut decoder = TransactionDecoder::new(42, Signature::default(), &tx);
        decoder
            .add_cpi_event_authority_and_program_id(key(5), key(6))
            .unwrap();
        let events = crate::value::AnchorCPIEvents::<GMSOLCPIEvent>::decode(decoder).unwrap();
        let GMSOLCPIEvent::OrderRemoved(decoded) = events.events()[0].data() else {
            panic!("expected typed OrderRemoved")
        };
        assert_eq!(decoded.try_to_vec().unwrap(), event.try_to_vec().unwrap());
    }
}
