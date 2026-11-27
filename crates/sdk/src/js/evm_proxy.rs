use crate::{evm_proxy::CancelIntent, pda::EvmAddressBytes, serde::StringPubkey};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

const EVM_ADDRESS_LEN: usize = std::mem::size_of::<EvmAddressBytes>();
const EVM_ADDRESS_HEX_LEN: usize = EVM_ADDRESS_LEN * 2;

/// An EVM proxy program PDA and its canonical bump.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ProxyAddress {
    /// Base58 derived account address.
    pub address: String,
    /// Canonical bump used to derive the account.
    pub bump: u8,
}

/// Arguments for deriving a proxy account address.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub struct FindProxyAddressArgs {
    /// Exactly 20 address bytes in hex, optionally prefixed by 0x or 0X.
    pub evm_address: String,
    /// Proxy program deployment; defaults to the declared program ID.
    #[serde(default)]
    pub program_id: Option<StringPubkey>,
}

fn parse_evm_address(input: &str) -> crate::Result<EvmAddressBytes> {
    let hex_address = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    if hex_address.len() != EVM_ADDRESS_HEX_LEN
        || !hex_address.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(crate::Error::custom(
            "expected a 20-byte EVM address in hex",
        ));
    }
    let mut address = EvmAddressBytes::default();
    hex::decode_to_slice(hex_address, &mut address).map_err(crate::Error::custom)?;
    Ok(address)
}

/// Derive a proxy PDA from raw EVM address bytes encoded in hex.
#[wasm_bindgen]
pub fn find_proxy_address(args: FindProxyAddressArgs) -> crate::Result<ProxyAddress> {
    let address_bytes = parse_evm_address(&args.evm_address)?;
    let program_id = args
        .program_id
        .map(Into::into)
        .unwrap_or(gmsol_programs::gmsol_evm_proxy::ID);
    let (address, bump) = crate::pda::find_proxy_address(&address_bytes, &program_id);
    Ok(ProxyAddress {
        address: address.to_string(),
        bump,
    })
}

/// Arguments for deriving the singleton Config PDA.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct FindProxyConfigAddressArgs {
    /// Deployment program ID; omitted for the declared deployment.
    #[serde(default)]
    pub program_id: Option<StringPubkey>,
}

/// Derive the selected deployment's singleton Config PDA.
#[wasm_bindgen]
pub fn find_proxy_config_address(args: FindProxyConfigAddressArgs) -> crate::Result<ProxyAddress> {
    let program_id = args
        .program_id
        .map(|id| id.0)
        .unwrap_or(gmsol_programs::gmsol_evm_proxy::ID);
    let (address, bump) = crate::pda::find_proxy_config_address(&program_id);
    Ok(ProxyAddress {
        address: address.to_string(),
        bump,
    })
}

pub(super) fn parse_hex<const N: usize>(input: &str, name: &str) -> crate::Result<[u8; N]> {
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

/// Signed Cancel fields and the selected proxy deployment.
#[derive(Clone, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub struct EvmProxyCancelIntentArgs {
    /// Exactly 20 EVM address bytes in hex, optionally prefixed by 0x.
    evm_address: String,
    /// Full-width 32-byte big-endian chain ID in hex.
    chain_id: String,
    /// Unsigned uint64 nonce as a decimal string.
    nonce: String,
    /// Unix execution deadline as a uint64 decimal string.
    deadline: String,
    /// Proxy program deployment; defaults to the declared program ID.
    #[serde(default)]
    program_id: Option<StringPubkey>,
}

impl EvmProxyCancelIntentArgs {
    pub(super) fn parse(&self) -> crate::Result<(Pubkey, CancelIntent)> {
        let program = self
            .program_id
            .as_ref()
            .map(|value| value.0)
            .unwrap_or(gmsol_programs::gmsol_evm_proxy::ID);
        Ok((
            program,
            CancelIntent {
                evm_address: parse_evm_address(&self.evm_address)?,
                chain_id: parse_hex(&self.chain_id, "chainId")?,
                nonce: self.nonce.parse().map_err(crate::Error::custom)?,
                deadline: self.deadline.parse().map_err(crate::Error::custom)?,
            },
        ))
    }
}

/// Cancel fields plus the immutable domain used for EVM signing.
#[derive(Clone, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub struct EvmProxyCancelSigningArgs {
    /// Signed fields, flattened into the signing argument object.
    #[serde(flatten)]
    intent: EvmProxyCancelIntentArgs,
    /// Base58 Store account bound to this deployment.
    store: StringPubkey,
    /// Exactly 32 cluster genesis hash bytes in hex.
    genesis_hash: String,
}

/// EIP-712 Cancel signing payload.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct EvmProxyCancelSigningPayload {
    domain: Eip712Domain,
    types: EvmProxyCancelTypes,
    #[serde(rename = "primaryType")]
    primary_type: &'static str,
    message: EvmProxyCancelMessage,
}

/// EIP-712 deployment domain.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct Eip712Domain {
    name: &'static str,
    version: &'static str,
    #[serde(rename = "chainId")]
    chain_id: String,
    salt: String,
}

/// Canonical signed Cancel fields.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct EvmProxyCancelMessage {
    #[serde(rename = "proxyAccount")]
    proxy_account: String,
    nonce: String,
    deadline: String,
}

/// One EIP-712 type field.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct TypedField {
    name: &'static str,
    #[serde(rename = "type")]
    field_type: &'static str,
}

/// EIP-712 type definitions for Cancel.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct EvmProxyCancelTypes {
    #[serde(rename = "EIP712Domain")]
    eip712_domain: Vec<TypedField>,
    #[serde(rename = "Cancel")]
    cancel: Vec<TypedField>,
}

/// Return viem-compatible EIP-712 data without converting uint64 values to JS numbers.
#[wasm_bindgen]
pub fn build_evm_proxy_cancel_typed_data(
    args: EvmProxyCancelSigningArgs,
) -> crate::Result<EvmProxyCancelSigningPayload> {
    let (program, intent) = args.intent.parse()?;
    let store = args.store.0;
    let genesis = parse_hex(&args.genesis_hash, "genesisHash")?;
    let data = intent.typed_data(&program, &store, &genesis);
    let fields = |items: &[(&'static str, &'static str)]| {
        items
            .iter()
            .map(|(name, field_type)| TypedField { name, field_type })
            .collect()
    };
    Ok(EvmProxyCancelSigningPayload {
        domain: Eip712Domain {
            name: data.name,
            version: data.version,
            chain_id: format!("0x{}", hex::encode(data.chain_id)),
            salt: format!("0x{}", hex::encode(data.salt)),
        },
        types: EvmProxyCancelTypes {
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
        message: EvmProxyCancelMessage {
            proxy_account: format!("0x{}", hex::encode(data.proxy_account.to_bytes())),
            nonce: data.nonce.to_string(),
            deadline: data.deadline.to_string(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_payload_keeps_full_width_nonce_strings() {
        for nonce in ["0", "9007199254740993", "18446744073709551615"] {
            let payload = build_evm_proxy_cancel_typed_data(EvmProxyCancelSigningArgs {
                intent: EvmProxyCancelIntentArgs {
                    evm_address: "0x1111111111111111111111111111111111111111".into(),
                    chain_id: hex::encode([1; 32]),
                    nonce: nonce.into(),
                    deadline: "1700000120".into(),
                    program_id: None,
                },
                store: Pubkey::new_unique().into(),
                genesis_hash: hex::encode([2; 32]),
            })
            .unwrap();
            assert_eq!(payload.message.nonce, nonce);
            assert_eq!(payload.message.deadline, "1700000120");
            assert_eq!(payload.primary_type, "Cancel");
            let encoded = serde_json::to_value(&payload).unwrap();
            assert!(encoded["domain"].get("chainId").is_some());
            assert!(encoded["message"].get("proxyAccount").is_some());
            assert_eq!(encoded["primaryType"], "Cancel");
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
    fn config_address_uses_selected_deployment() {
        const EXPECTED_CONFIG_BUMP: u8 = 252;
        let default =
            find_proxy_config_address(FindProxyConfigAddressArgs { program_id: None }).unwrap();
        assert_eq!(
            default.address,
            "E13dsYehufSsFWGktSYNREVHnogUsrDeJQH6swaL5pJA"
        );
        assert_eq!(default.bump, EXPECTED_CONFIG_BUMP);
        let program = Pubkey::new_unique();
        let selected = find_proxy_config_address(FindProxyConfigAddressArgs {
            program_id: Some(program.into()),
        })
        .unwrap();
        let expected = crate::pda::find_proxy_config_address(&program);
        assert_eq!(
            (selected.address, selected.bump),
            (expected.0.to_string(), expected.1)
        );
    }

    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn proxy_address_hex_variants_match_rust_derivation() {
        let input = "0x0123456789abcdef0123456789abcdef01234567";
        let bytes = parse_evm_address(input).unwrap();
        let expected = crate::pda::find_proxy_address(&bytes, &gmsol_programs::gmsol_evm_proxy::ID);
        let result = find_proxy_address(FindProxyAddressArgs {
            evm_address: input.into(),
            program_id: None,
        })
        .unwrap();
        assert_eq!(result.address, expected.0.to_string());
        assert_eq!(result.bump, expected.1);
        for variant in [
            "0123456789ABCDEF0123456789ABCDEF01234567",
            "0X0123456789ABCDEF0123456789ABCDEF01234567",
        ] {
            assert_eq!(
                find_proxy_address(FindProxyAddressArgs {
                    evm_address: variant.into(),
                    program_id: None,
                })
                .unwrap()
                .address,
                result.address
            );
        }
        let alternate_program = Pubkey::new_unique();
        let alternate = find_proxy_address(FindProxyAddressArgs {
            evm_address: input.into(),
            program_id: Some(alternate_program.into()),
        })
        .unwrap();
        let alternate_expected = crate::pda::find_proxy_address(&bytes, &alternate_program);
        assert_eq!(alternate.address, alternate_expected.0.to_string());
        assert_eq!(alternate.bump, alternate_expected.1);
        assert_ne!(alternate.address, result.address);
    }

    #[test]
    fn proxy_address_rejects_malformed_hex() {
        for invalid in [
            "",
            "0x1234",
            "0xgg23456789abcdef0123456789abcdef01234567",
            " 0x0123456789abcdef0123456789abcdef01234567",
            "0x0123456789abcdef0123456789abcdef01234567 ",
            "0x0x0123456789abcdef0123456789abcdef01234567",
        ] {
            assert!(
                find_proxy_address(FindProxyAddressArgs {
                    evm_address: invalid.into(),
                    program_id: None,
                })
                .is_err(),
                "accepted {invalid:?}"
            );
        }
    }
}
