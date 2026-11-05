use std::str::FromStr;

use super::instructions::TransactionGroup;
use crate::evm_proxy::{cancel_instruction, CancelIntent};
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
    generation: String,
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
                generation: self.generation.parse().map_err(crate::Error::custom)?,
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
    generation: String,
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
            .map(|(name, field_type)| CancelField {
                name: *name,
                field_type: *field_type,
            })
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
                ("generation", "uint64"),
                ("nonce", "uint64"),
                ("deadline", "uint64"),
            ]),
        },
        primary_type: "Cancel",
        message: CancelMessage {
            proxy_account: format!("0x{}", hex::encode(data.proxy_account.to_bytes())),
            generation: data.generation.to_string(),
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
