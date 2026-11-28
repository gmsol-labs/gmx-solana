//! Intent encoding compared with alloy's EIP-712 implementation.

use std::str::FromStr;

use alloy_primitives::{keccak256, B256, U256};
use alloy_sol_types::{Eip712Domain, SolStruct};
use anchor_lang::{prelude::Pubkey, solana_program::keccak};
use gmsol_evm_proxy_intent::{digest, u64_word, Cancel, Domain, Intent, DOMAIN_TYPE};
use serde_json::Value;

mod reference {
    alloy_sol_types::sol! {
        struct Cancel {
            bytes32 proxyAccount;
            uint64 nonce;
            uint64 deadline;
        }
    }
}

const RANDOM_CASES: usize = 16;

fn reference_domain(domain: &Domain) -> Eip712Domain {
    let salt = keccak256(
        [
            domain.program.as_ref(),
            domain.store.as_ref(),
            &domain.genesis_hash,
        ]
        .concat(),
    );
    Eip712Domain::new(
        Some("GMX Solana EVM Proxy".into()),
        Some("1".into()),
        Some(U256::from_be_bytes(domain.chain_id)),
        None,
        Some(salt),
    )
}

fn reference_cancel(cancel: &Cancel) -> reference::Cancel {
    reference::Cancel {
        proxyAccount: B256::from(cancel.proxy_account.to_bytes()),
        nonce: cancel.nonce,
        deadline: cancel.deadline,
    }
}

fn case(bytes: impl Fn(&str) -> [u8; 32], number: impl Fn(&str) -> u64) -> (Domain, Cancel) {
    let domain = Domain {
        program: Pubkey::new_from_array(bytes("program")),
        store: Pubkey::new_from_array(bytes("store")),
        genesis_hash: bytes("genesis_hash"),
        chain_id: bytes("chain_id"),
    };
    let cancel = Cancel {
        proxy_account: Pubkey::new_from_array(bytes("proxy_account")),
        nonce: number("nonce"),
        deadline: number("deadline"),
    };
    (domain, cancel)
}

fn cases() -> Vec<(Domain, Cancel)> {
    let boundaries =
        [(0, 0), (u8::MAX, u64::MAX)].map(|(byte, number)| case(|_| [byte; 32], |_| number));
    let random = (0..RANDOM_CASES).map(|index| {
        let bytes = move |field: &str| keccak::hash(format!("{field}/{index}").as_bytes()).0;
        case(bytes, move |field| {
            u64::from_be_bytes(bytes(field)[..8].try_into().unwrap())
        })
    });
    boundaries.into_iter().chain(random).collect()
}

#[test]
fn type_strings_match_alloy() {
    let (domain, _) = case(|_| [0; 32], |_| 0);
    assert_eq!(DOMAIN_TYPE, reference_domain(&domain).encode_type());
    assert_eq!(Cancel::TYPE, reference::Cancel::eip712_encode_type());
}

#[test]
fn cancel_encoding_matches_alloy() {
    for (domain, cancel) in cases() {
        let reference_domain = reference_domain(&domain);
        let reference_cancel = reference_cancel(&cancel);
        assert_eq!(domain.separator(), reference_domain.separator().0);
        assert_eq!(
            cancel.struct_hash(),
            reference_cancel.eip712_hash_struct().0
        );
        assert_eq!(
            digest(&domain, &cancel),
            reference_cancel.eip712_signing_hash(&reference_domain).0
        );
    }
}

#[test]
fn shared_cancel_vector_matches_the_crate_and_alloy() {
    let vector: Value = serde_json::from_str(include_str!("../fixtures/cancel.json")).unwrap();
    let pubkey = |key: &str| Pubkey::from_str(vector[key].as_str().unwrap()).unwrap();
    let number = |key: &str| vector[key].as_str().unwrap().parse::<u64>().unwrap();
    let bytes = |key: &str| -> [u8; 32] {
        hex::decode(vector[key].as_str().unwrap().strip_prefix("0x").unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    };
    let domain = Domain {
        program: pubkey("program"),
        store: pubkey("store"),
        genesis_hash: bytes("genesisHash"),
        chain_id: u64_word(number("chainId")),
    };
    let cancel = Cancel {
        proxy_account: pubkey("proxyAccount"),
        nonce: number("nonce"),
        deadline: number("deadline"),
    };
    let expected = bytes("digest");
    assert_eq!(digest(&domain, &cancel), expected);
    assert_eq!(
        reference_cancel(&cancel)
            .eip712_signing_hash(&reference_domain(&domain))
            .0,
        expected
    );
}
