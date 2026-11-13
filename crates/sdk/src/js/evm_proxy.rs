use std::str::FromStr;

use super::instructions::TransactionGroup;
use crate::evm_proxy::{
    cancel_instruction, set_referral_code_instruction, withdraw_instruction, CancelIntent,
    SetReferralCodeIntent, WithdrawIntent,
};
use serde::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Proxy PDA and its canonical bump.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ProxyAddress {
    pub address: String,
    pub bump: u8,
}

fn parse_evm_address(input: &str) -> crate::Result<[u8; 20]> {
    let hex_address = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    if hex_address.len() != 40 || !hex_address.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(crate::Error::custom(
            "expected a 20-byte EVM address in hex",
        ));
    }
    let mut address = [0_u8; 20];
    hex::decode_to_slice(hex_address, &mut address).map_err(crate::Error::custom)?;
    Ok(address)
}

fn parse_hex<const N: usize>(input: &str, name: &str) -> crate::Result<[u8; N]> {
    let value = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(crate::Error::custom(format!(
            "{name} must contain {N} hex bytes"
        )));
    }
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).map_err(crate::Error::custom)?;
    Ok(bytes)
}

/// Fields shared by the signable Cancel payload and its Solana instruction.
#[derive(Clone, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CancelIntentArgs {
    evm_address: String,
    chain_id: String,
    nonce: String,
    deadline: String,
    store: String,
    genesis_hash: String,
    #[serde(default)]
    program_id: Option<String>,
}

impl CancelIntentArgs {
    fn parse(&self) -> crate::Result<(Pubkey, Pubkey, [u8; 32], CancelIntent)> {
        let program = match &self.program_id {
            Some(value) => Pubkey::from_str(value)?,
            None => gmsol_programs::gmsol_evm_proxy::ID,
        };
        Ok((
            program,
            Pubkey::from_str(&self.store)?,
            parse_hex(&self.genesis_hash, "genesisHash")?,
            CancelIntent {
                evm_address: parse_evm_address(&self.evm_address)?,
                chain_id: parse_hex(&self.chain_id, "chainId")?,
                nonce: self.nonce.parse().map_err(crate::Error::custom)?,
                deadline: self.deadline.parse().map_err(crate::Error::custom)?,
            },
        ))
    }
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct CancelSigningPayload {
    domain: CancelDomain,
    types: CancelTypes,
    #[serde(rename = "primaryType")]
    primary_type: &'static str,
    message: CancelMessage,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CancelDomain {
    name: &'static str,
    version: &'static str,
    chain_id: String,
    salt: String,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CancelMessage {
    proxy_account: String,
    nonce: String,
    deadline: String,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct CancelField {
    name: &'static str,
    #[serde(rename = "type")]
    field_type: &'static str,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct CancelTypes {
    #[serde(rename = "EIP712Domain")]
    eip712_domain: Vec<CancelField>,
    #[serde(rename = "Cancel")]
    cancel: Vec<CancelField>,
}

/// Return viem-compatible EIP-712 data without converting uint64 values to JS numbers.
#[wasm_bindgen(js_name = buildCancelTypedData)]
pub fn build_cancel_typed_data(args: CancelIntentArgs) -> crate::Result<CancelSigningPayload> {
    let (program, store, genesis, intent) = args.parse()?;
    let data = intent.typed_data(&program, &store, &genesis);
    let fields = |items: &[(&'static str, &'static str)]| {
        items
            .iter()
            .map(|(name, field_type)| CancelField { name, field_type })
            .collect()
    };
    Ok(CancelSigningPayload {
        domain: CancelDomain {
            name: data.name,
            version: data.version,
            chain_id: format!("0x{}", hex::encode(data.chain_id)),
            salt: format!("0x{}", hex::encode(data.salt)),
        },
        types: CancelTypes {
            eip712_domain: fields(&[
                ("name", "string"),
                ("version", "string"),
                ("chainId", "uint256"),
                ("salt", "bytes32"),
            ]),
            cancel: fields(&[
                ("proxyAccount", "bytes32"),
                ("nonce", "uint64"),
                ("deadline", "uint64"),
            ]),
        },
        primary_type: "Cancel",
        message: CancelMessage {
            proxy_account: format!("0x{}", hex::encode(data.proxy_account.to_bytes())),
            nonce: data.nonce.to_string(),
            deadline: data.deadline.to_string(),
        },
    })
}

/// Build an unsigned Solana Cancel transaction for an arbitrary fee payer.
#[wasm_bindgen(js_name = buildCancelTransaction)]
pub fn build_cancel_transaction(
    args: CancelIntentArgs,
    payer: &str,
    recent_blockhash: &str,
    signature: &str,
    recovery_id: u8,
) -> crate::Result<TransactionGroup> {
    let (program, _, _, intent) = args.parse()?;
    let payer = Pubkey::from_str(payer)?;
    let ix = cancel_instruction(
        program,
        payer,
        &intent,
        parse_hex(signature, "signature")?,
        recovery_id,
    )?;
    Ok(TransactionGroup::from_single_instruction(
        ix,
        payer,
        Hash::from_str(recent_blockhash).map_err(crate::Error::custom)?,
    ))
}

/// Inputs shared by the signable payload and Solana transaction builder.
#[derive(Clone, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawIntentArgs {
    evm_address: String,
    chain_id: String,
    mint: String,
    amount: String,
    decimals: u8,
    recipient: String,
    nonce: String,
    deadline: String,
    store: String,
    genesis_hash: String,
    #[serde(default)]
    program_id: Option<String>,
}

impl WithdrawIntentArgs {
    fn parse(&self) -> crate::Result<(Pubkey, Pubkey, [u8; 32], WithdrawIntent)> {
        let program = match &self.program_id {
            Some(value) => Pubkey::from_str(value)?,
            None => gmsol_programs::gmsol_evm_proxy::ID,
        };
        let store = Pubkey::from_str(&self.store)?;
        let genesis = parse_hex(&self.genesis_hash, "genesisHash")?;
        let intent = WithdrawIntent {
            evm_address: parse_evm_address(&self.evm_address)?,
            chain_id: parse_hex(&self.chain_id, "chainId")?,
            mint: Pubkey::from_str(&self.mint)?,
            amount: self.amount.parse().map_err(crate::Error::custom)?,
            decimals: self.decimals,
            recipient: Pubkey::from_str(&self.recipient)?,
            nonce: self.nonce.parse().map_err(crate::Error::custom)?,
            deadline: self.deadline.parse().map_err(crate::Error::custom)?,
        };
        let proxy = crate::pda::find_proxy_address(&intent.evm_address, &program).0;
        if intent.amount == 0 || intent.recipient == proxy {
            return Err(crate::Error::custom("invalid Withdraw amount or recipient"));
        }
        Ok((program, store, genesis, intent))
    }
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct TypedField {
    name: String,
    #[serde(rename = "type")]
    field_type: String,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct WithdrawTypes {
    #[serde(rename = "EIP712Domain")]
    eip712_domain: Vec<TypedField>,
    #[serde(rename = "Withdraw")]
    withdraw: Vec<TypedField>,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawDomain {
    name: &'static str,
    version: &'static str,
    chain_id: String,
    salt: String,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawMessage {
    proxy_account: String,
    mint: String,
    amount: String,
    decimals: u8,
    recipient: String,
    nonce: String,
    deadline: String,
}

/// viem-compatible EIP-712 object. Numeric strings are typed as uints below.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawSigningPayload {
    domain: WithdrawDomain,
    types: WithdrawTypes,
    #[serde(rename = "primaryType")]
    primary_type: &'static str,
    message: WithdrawMessage,
}

fn fields(items: &[(&str, &str)]) -> Vec<TypedField> {
    items
        .iter()
        .map(|(name, field_type)| TypedField {
            name: (*name).to_string(),
            field_type: (*field_type).to_string(),
        })
        .collect()
}

/// Construct the exact signable Withdraw typed data without JS Number conversion.
#[wasm_bindgen(js_name = buildWithdrawTypedData)]
pub fn build_withdraw_typed_data(
    args: WithdrawIntentArgs,
) -> crate::Result<WithdrawSigningPayload> {
    let (program, store, genesis, intent) = args.parse()?;
    let data = intent.typed_data(&program, &store, &genesis);
    let types = WithdrawTypes {
        eip712_domain: fields(&[
            ("name", "string"),
            ("version", "string"),
            ("chainId", "uint256"),
            ("salt", "bytes32"),
        ]),
        withdraw: fields(&[
            ("proxyAccount", "bytes32"),
            ("mint", "string"),
            ("amount", "uint64"),
            ("decimals", "uint8"),
            ("recipient", "string"),
            ("nonce", "uint64"),
            ("deadline", "uint64"),
        ]),
    };
    Ok(WithdrawSigningPayload {
        domain: WithdrawDomain {
            name: data.name,
            version: data.version,
            chain_id: format!("0x{}", hex::encode(data.chain_id)),
            salt: format!("0x{}", hex::encode(data.salt)),
        },
        types,
        primary_type: "Withdraw",
        message: WithdrawMessage {
            proxy_account: format!("0x{}", hex::encode(data.proxy_account.to_bytes())),
            mint: data.mint,
            amount: data.amount.to_string(),
            decimals: data.decimals,
            recipient: data.recipient,
            nonce: data.nonce.to_string(),
            deadline: data.deadline.to_string(),
        },
    })
}

/// Build one unsigned Solana transaction for an arbitrary fee payer and signed intent.
#[wasm_bindgen(js_name = buildWithdrawTransaction)]
pub fn build_withdraw_transaction(
    args: WithdrawIntentArgs,
    payer: &str,
    recent_blockhash: &str,
    signature: &str,
    recovery_id: u8,
) -> crate::Result<TransactionGroup> {
    let (program, _, _, intent) = args.parse()?;
    let ix = withdraw_instruction(
        program,
        Pubkey::from_str(payer)?,
        &intent,
        parse_hex(signature, "signature")?,
        recovery_id,
    )?;
    Ok(TransactionGroup::from_single_instruction(
        ix,
        Pubkey::from_str(payer)?,
        Hash::from_str(recent_blockhash).map_err(crate::Error::custom)?,
    ))
}

/// Referral input accepts Store base58 aliases and normalizes before signing.
#[derive(Clone, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct SetReferralCodeIntentArgs {
    evm_address: String,
    chain_id: String,
    code: String,
    nonce: String,
    deadline: String,
    store: String,
    genesis_hash: String,
    #[serde(default)]
    program_id: Option<String>,
}

impl SetReferralCodeIntentArgs {
    fn parse(&self) -> crate::Result<(Pubkey, Pubkey, [u8; 32], SetReferralCodeIntent)> {
        let program = match &self.program_id {
            Some(value) => Pubkey::from_str(value)?,
            None => gmsol_programs::gmsol_evm_proxy::ID,
        };
        let code = gmsol_programs::gmsol_store::accounts::ReferralCodeV2::decode(&self.code)?;
        if code == [0; 8] {
            return Err(crate::Error::custom("referral code cannot be zero"));
        }
        Ok((
            program,
            Pubkey::from_str(&self.store)?,
            parse_hex(&self.genesis_hash, "genesisHash")?,
            SetReferralCodeIntent {
                evm_address: parse_evm_address(&self.evm_address)?,
                chain_id: parse_hex(&self.chain_id, "chainId")?,
                code,
                nonce: self.nonce.parse().map_err(crate::Error::custom)?,
                deadline: self.deadline.parse().map_err(crate::Error::custom)?,
            },
        ))
    }
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SetReferralCodeSigningPayload {
    domain: WithdrawDomain,
    types: SetReferralCodeTypes,
    #[serde(rename = "primaryType")]
    primary_type: &'static str,
    message: SetReferralCodeMessage,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SetReferralCodeTypes {
    #[serde(rename = "EIP712Domain")]
    eip712_domain: Vec<TypedField>,
    #[serde(rename = "SetReferralCode")]
    set_referral_code: Vec<TypedField>,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct SetReferralCodeMessage {
    proxy_account: String,
    code: String,
    nonce: String,
    deadline: String,
}

/// Sign this normalized payload, not the verbatim input alias.
#[wasm_bindgen(js_name = buildSetReferralCodeTypedData)]
pub fn build_set_referral_code_typed_data(
    args: SetReferralCodeIntentArgs,
) -> crate::Result<SetReferralCodeSigningPayload> {
    let (program, store, genesis, intent) = args.parse()?;
    let data = intent.typed_data(&program, &store, &genesis);
    Ok(SetReferralCodeSigningPayload {
        domain: WithdrawDomain {
            name: data.name,
            version: data.version,
            chain_id: format!("0x{}", hex::encode(data.chain_id)),
            salt: format!("0x{}", hex::encode(data.salt)),
        },
        types: SetReferralCodeTypes {
            eip712_domain: fields(&[
                ("name", "string"),
                ("version", "string"),
                ("chainId", "uint256"),
                ("salt", "bytes32"),
            ]),
            set_referral_code: fields(&[
                ("proxyAccount", "bytes32"),
                ("code", "string"),
                ("nonce", "uint64"),
                ("deadline", "uint64"),
            ]),
        },
        primary_type: "SetReferralCode",
        message: SetReferralCodeMessage {
            proxy_account: format!("0x{}", hex::encode(data.proxy_account.to_bytes())),
            code: data.code,
            nonce: data.nonce.to_string(),
            deadline: data.deadline.to_string(),
        },
    })
}

/// Build an unsigned transaction with separate rent sponsor and fee payer.
/// Both accounts must sign when their keys differ.
#[wasm_bindgen(js_name = buildSetReferralCodeTransaction)]
pub fn build_set_referral_code_transaction(
    args: SetReferralCodeIntentArgs,
    submitter: &str,
    payer: &str,
    recent_blockhash: &str,
    signature: &str,
    recovery_id: u8,
) -> crate::Result<TransactionGroup> {
    let (program, store, _, intent) = args.parse()?;
    let ix = set_referral_code_instruction(
        program,
        Pubkey::from_str(submitter)?,
        store,
        &intent,
        parse_hex(signature, "signature")?,
        recovery_id,
    )?;
    Ok(TransactionGroup::from_single_instruction(
        ix,
        Pubkey::from_str(payer)?,
        Hash::from_str(recent_blockhash).map_err(crate::Error::custom)?,
    ))
}

/// Derive a proxy PDA from exactly 20 EVM address bytes in hex.
/// An omitted program ID uses the declared proxy program.
#[wasm_bindgen(js_name = findProxyAddress)]
pub fn find_proxy_address(
    evm_address: &str,
    program_id: Option<String>,
) -> crate::Result<ProxyAddress> {
    let address_bytes = parse_evm_address(evm_address)?;
    let program_id = match program_id {
        Some(value) => Pubkey::from_str(&value)?,
        None => gmsol_programs::gmsol_evm_proxy::ID,
    };
    let (address, bump) = crate::pda::find_proxy_address(&address_bytes, &program_id);
    Ok(ProxyAddress {
        address: address.to_string(),
        bump,
    })
}

/// Derive the singleton Config PDA with the Rust SDK seed helper.
#[wasm_bindgen(js_name = findProxyConfigAddress)]
pub fn find_proxy_config_address(program_id: Option<String>) -> crate::Result<ProxyAddress> {
    let program_id = match program_id {
        Some(value) => Pubkey::from_str(&value)?,
        None => gmsol_programs::gmsol_evm_proxy::ID,
    };
    let (address, bump) = crate::pda::find_proxy_config_address(&program_id);
    Ok(ProxyAddress {
        address: address.to_string(),
        bump,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::AnchorDeserialize;

    #[test]
    fn referral_aliases_and_full_width_nonce_are_normalized_before_signing() {
        let args = SetReferralCodeIntentArgs {
            evm_address: hex::encode([0x17; 20]),
            chain_id: hex::encode([1; 32]),
            code: "2".into(),
            nonce: "9007199254740993".into(),
            deadline: u64::MAX.to_string(),
            store: Pubkey::new_unique().to_string(),
            genesis_hash: hex::encode([2; 32]),
            program_id: None,
        };
        for alias in ["2", "12", "11111112"] {
            for nonce in ["0", "9007199254740993", "18446744073709551615"] {
                let input = SetReferralCodeIntentArgs {
                    code: alias.into(),
                    nonce: nonce.into(),
                    ..args.clone()
                };
                let payload = build_set_referral_code_typed_data(input).unwrap();
                assert_eq!(payload.message.code, "2");
                assert_eq!(payload.message.nonce, nonce);
                assert_eq!(payload.message.deadline, u64::MAX.to_string());
                assert_eq!(payload.primary_type, "SetReferralCode");
                assert_eq!(
                    payload
                        .types
                        .set_referral_code
                        .iter()
                        .map(|f| (f.name.as_str(), f.field_type.as_str()))
                        .collect::<Vec<_>>(),
                    [
                        ("proxyAccount", "bytes32"),
                        ("code", "string"),
                        ("nonce", "uint64"),
                        ("deadline", "uint64")
                    ]
                );
            }
        }
        for invalid in ["", "0", "O", "I", "l", "!", "111111111", "1", "11111111"] {
            assert!(
                build_set_referral_code_typed_data(SetReferralCodeIntentArgs {
                    code: invalid.into(),
                    ..args.clone()
                })
                .is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn referral_transaction_preserves_distinct_sponsor_and_fee_payer() {
        let sponsor = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let args = SetReferralCodeIntentArgs {
            evm_address: hex::encode([0x17; 20]),
            chain_id: hex::encode([1; 32]),
            code: "12".into(),
            nonce: u64::MAX.to_string(),
            deadline: "1700000120".into(),
            store: Pubkey::new_unique().to_string(),
            genesis_hash: hex::encode([2; 32]),
            program_id: None,
        };
        let transaction = build_set_referral_code_transaction(
            args,
            &sponsor.to_string(),
            &payer.to_string(),
            &Hash::default().to_string(),
            &hex::encode([3; 64]),
            1,
        )
        .unwrap();
        let wire: Vec<Vec<Vec<u8>>> =
            bincode::deserialize(&bincode::serialize(&transaction.serialize().unwrap()).unwrap())
                .unwrap();
        let transaction: solana_sdk::transaction::VersionedTransaction =
            bincode::deserialize(&wire[0][0]).unwrap();
        assert_eq!(transaction.signatures.len(), 2);
        let solana_sdk::message::VersionedMessage::Legacy(message) = transaction.message else {
            panic!("expected legacy transaction")
        };
        assert_eq!(message.account_keys[0], payer);
        assert!(message.account_keys[..2].contains(&sponsor));
        let args = gmsol_programs::gmsol_evm_proxy::client::args::SetReferralCode::deserialize(
            &mut &message.instructions[0].data[8..],
        )
        .unwrap();
        assert_eq!(args.code, [0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(args.nonce, u64::MAX);
    }

    #[test]
    fn cancel_payload_keeps_full_width_single_nonce_strings() {
        for nonce in ["0", "9007199254740993", "18446744073709551615"] {
            let payload = build_cancel_typed_data(CancelIntentArgs {
                evm_address: "0x1111111111111111111111111111111111111111".into(),
                chain_id: hex::encode([1; 32]),
                nonce: nonce.into(),
                deadline: "1700000120".into(),
                store: Pubkey::new_unique().to_string(),
                genesis_hash: hex::encode([2; 32]),
                program_id: None,
            })
            .unwrap();
            assert_eq!(payload.message.nonce, nonce);
            assert_eq!(payload.message.deadline, "1700000120");
            assert_eq!(payload.primary_type, "Cancel");
            assert_eq!(
                payload
                    .types
                    .cancel
                    .iter()
                    .map(|field| (field.name, field.field_type))
                    .collect::<Vec<_>>(),
                vec![
                    ("proxyAccount", "bytes32"),
                    ("nonce", "uint64"),
                    ("deadline", "uint64")
                ]
            );
        }
    }

    #[test]
    fn config_address_matches_rust_derivation() {
        let program_id = gmsol_programs::gmsol_evm_proxy::ID;
        let (address, bump) = crate::pda::find_proxy_config_address(&program_id);
        let result = find_proxy_config_address(None).unwrap();
        assert_eq!((result.address, result.bump), (address.to_string(), bump));
        assert_eq!(
            address.to_string(),
            "E13dsYehufSsFWGktSYNREVHnogUsrDeJQH6swaL5pJA"
        );
        assert_eq!(bump, 252);
        assert!(find_proxy_config_address(Some("invalid program".into())).is_err());
    }

    #[test]
    fn proxy_address_hex_variants_match_rust_derivation() {
        let input = "0x0123456789abcdef0123456789abcdef01234567";
        let bytes = parse_evm_address(input).unwrap();
        let expected = crate::pda::find_proxy_address(&bytes, &gmsol_programs::gmsol_evm_proxy::ID);
        let result = find_proxy_address(input, None).unwrap();
        assert_eq!(result.address, expected.0.to_string());
        assert_eq!(result.bump, expected.1);
        for variant in [
            "0123456789ABCDEF0123456789ABCDEF01234567",
            "0X0123456789ABCDEF0123456789ABCDEF01234567",
        ] {
            assert_eq!(
                find_proxy_address(variant, None).unwrap().address,
                result.address
            );
        }
        let alternate_program = Pubkey::new_unique();
        let alternate = find_proxy_address(input, Some(alternate_program.to_string())).unwrap();
        let alternate_expected = crate::pda::find_proxy_address(&bytes, &alternate_program);
        assert_eq!(alternate.address, alternate_expected.0.to_string());
        assert_eq!(alternate.bump, alternate_expected.1);
        assert_ne!(alternate.address, result.address);
    }

    #[test]
    fn withdraw_js_builders_preserve_signed_numbers_and_fee_payer() {
        let payer = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let store = Pubkey::new_unique();
        let args = WithdrawIntentArgs {
            evm_address: "0x0123456789abcdef0123456789abcdef01234567".to_string(),
            chain_id: format!("0x{}", hex::encode([0x12; 32])),
            mint: mint.to_string(),
            amount: u64::MAX.to_string(),
            decimals: 6,
            recipient: recipient.to_string(),
            nonce: "0".to_string(),
            deadline: "1700000120".to_string(),
            store: store.to_string(),
            genesis_hash: format!("0x{}", hex::encode([0x42; 32])),
            program_id: None,
        };
        let payload = build_withdraw_typed_data(args.clone()).unwrap();
        assert_eq!(payload.primary_type, "Withdraw");
        let serialized = serde_json::to_value(&payload).unwrap();
        assert_eq!(serialized["primaryType"], "Withdraw");
        assert!(serialized.get("primary_type").is_none());
        assert_eq!(payload.message.amount, u64::MAX.to_string());
        assert_eq!(payload.message.nonce, "0");
        for nonce in ["0", "9007199254740993", "18446744073709551615"] {
            let typed = build_withdraw_typed_data(WithdrawIntentArgs {
                nonce: nonce.to_string(),
                ..args.clone()
            })
            .unwrap();
            assert_eq!(typed.message.nonce, nonce);
            assert_eq!(typed.types.withdraw.len(), 7);
            assert_eq!(typed.types.withdraw[5].name, "nonce");
            assert!(serde_json::to_value(typed).unwrap()["message"]
                .get("generation")
                .is_none());
        }
        assert_eq!(payload.message.mint, mint.to_string());
        assert_eq!(payload.message.recipient, recipient.to_string());
        assert_eq!(payload.domain.name, gmsol_evm_proxy_intent::DOMAIN_NAME);
        assert_eq!(
            payload.domain.salt,
            format!(
                "0x{}",
                hex::encode(gmsol_evm_proxy_intent::domain_salt(
                    &gmsol_programs::gmsol_evm_proxy::ID,
                    &store,
                    &[0x42; 32]
                ))
            )
        );
        assert_eq!(payload.types.withdraw[5].name, "nonce");

        let transaction = build_withdraw_transaction(
            WithdrawIntentArgs {
                amount: "42".to_string(),
                ..args
            },
            &payer.to_string(),
            &Hash::default().to_string(),
            &format!("0x{}", hex::encode([3; 64])),
            1,
        )
        .unwrap();
        let wire: Vec<Vec<Vec<u8>>> =
            bincode::deserialize(&bincode::serialize(&transaction.serialize().unwrap()).unwrap())
                .unwrap();
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0].len(), 1);
        let signed: solana_sdk::transaction::VersionedTransaction =
            bincode::deserialize(&wire[0][0]).unwrap();
        assert_eq!(signed.signatures.len(), 1);
        let solana_sdk::message::VersionedMessage::Legacy(message) = signed.message else {
            panic!("expected legacy transaction")
        };
        assert_eq!(message.account_keys[0], payer);
        assert_eq!(message.instructions.len(), 1);
        assert_eq!(
            message.account_keys[message.instructions[0].program_id_index as usize],
            gmsol_programs::gmsol_evm_proxy::ID
        );
    }

    #[test]
    fn proxy_address_rejects_malformed_hex_and_program_id() {
        for invalid in [
            "",
            "0x1234",
            "0xgg23456789abcdef0123456789abcdef01234567",
            " 0x0123456789abcdef0123456789abcdef01234567",
            "0x0123456789abcdef0123456789abcdef01234567 ",
            "0x0x0123456789abcdef0123456789abcdef01234567",
        ] {
            assert!(
                find_proxy_address(invalid, None).is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(find_proxy_address(
            "0x0123456789abcdef0123456789abcdef01234567",
            Some("invalid base58 program".into()),
        )
        .is_err());
    }
}
