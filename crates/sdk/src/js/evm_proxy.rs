use std::str::FromStr;

use serde::{Deserialize, Serialize};
use solana_sdk::{hash::Hash, pubkey::Pubkey};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use super::instructions::TransactionGroup;
use crate::evm_proxy::{withdraw_instruction, WithdrawIntent};

/// Proxy PDA and canonical bump.
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
    let bytes = hex::decode(hex_address).map_err(crate::Error::custom)?;
    bytes
        .try_into()
        .map_err(|_| crate::Error::custom("expected a 20-byte EVM address in hex"))
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
    hex::decode(value)
        .map_err(crate::Error::custom)?
        .try_into()
        .map_err(|_| crate::Error::custom(format!("{name} must contain {N} hex bytes")))
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
    generation: String,
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
            generation: self.generation.parse().map_err(crate::Error::custom)?,
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
    generation: String,
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
            ("generation", "uint64"),
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
            generation: data.generation.to_string(),
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

/// Derive the proxy PDA from an EVM address. An omitted program ID uses this SDK's proxy program.
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

/// Derive the singleton Config PDA. An omitted program ID uses this SDK's proxy program.
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

    #[test]
    fn derives_from_raw_evm_address() {
        let evm_address = "0x0123456789abcdef0123456789abcdef01234567";
        let parsed = parse_evm_address(evm_address).unwrap();
        let result = find_proxy_address(evm_address, None).unwrap();
        let expected =
            crate::pda::find_proxy_address(&parsed, &gmsol_programs::gmsol_evm_proxy::ID);
        assert_eq!(result.address, expected.0.to_string());
        assert_eq!(result.bump, expected.1);
        assert_eq!(
            find_proxy_address("0123456789ABCDEF0123456789ABCDEF01234567", None)
                .unwrap()
                .address,
            result.address
        );
    }

    #[test]
    fn rejects_malformed_address() {
        assert!(parse_evm_address("0x1234").is_err());
        assert!(parse_evm_address("0xgg23456789abcdef0123456789abcdef01234567").is_err());
    }

    #[test]
    fn derives_config_address() {
        let program_id = gmsol_programs::gmsol_evm_proxy::ID;
        let expected = crate::pda::find_proxy_config_address(&program_id);
        let result = find_proxy_config_address(None).unwrap();
        assert_eq!(result.address, expected.0.to_string());
        assert_eq!(result.bump, expected.1);
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
            generation: "8".to_string(),
            nonce: "0".to_string(),
            deadline: "1700000120".to_string(),
            store: store.to_string(),
            genesis_hash: format!("0x{}", hex::encode([0x42; 32])),
            program_id: None,
        };
        let payload = build_withdraw_typed_data(args.clone()).unwrap();
        assert_eq!(payload.primary_type, "Withdraw");
        assert_eq!(payload.message.amount, u64::MAX.to_string());
        assert_eq!(payload.message.generation, "8");
        assert_eq!(payload.message.nonce, "0");
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
        assert_eq!(payload.types.withdraw[5].name, "generation");

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
}
