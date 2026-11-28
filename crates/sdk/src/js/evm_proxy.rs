use std::collections::BTreeMap;

use crate::{builders::evm_proxy::EvmProxyProgram, pda::EvmAddressBytes, serde::StringPubkey};
use gmsol_evm_proxy_intent::{
    type_members, u64_word, Cancel, Domain, Intent, DOMAIN_NAME, DOMAIN_TYPE, DOMAIN_VERSION,
};
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
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes)
        .map_err(|error| crate::Error::custom(format!("{name}: {error}")))?;
    Ok(bytes)
}

fn hex_string(bytes: impl AsRef<[u8]>) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// Signed Cancel fields and the proxy deployment.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi, large_number_types_as_bigints)]
pub struct EvmProxyCancelArgs {
    /// EVM owner address, 20 bytes in hex.
    evm_address: String,
    /// EVM chain the intent is signed for.
    pub(super) chain_id: u64,
    /// Proxy nonce consumed by the intent.
    nonce: u64,
    /// Latest Unix timestamp at which the intent executes.
    deadline: u64,
    /// Proxy deployment; defaults to the declared program ID.
    #[serde(default)]
    program: Option<EvmProxyProgram>,
}

impl EvmProxyCancelArgs {
    pub(super) fn program(&self) -> EvmProxyProgram {
        self.program.clone().unwrap_or_default()
    }

    pub(super) fn intent(&self, program: &EvmProxyProgram) -> crate::Result<Cancel> {
        let evm_address = parse_evm_address(&self.evm_address)?;
        Ok(Cancel {
            proxy_account: crate::pda::find_proxy_address(&evm_address, &program.id).0,
            nonce: self.nonce,
            deadline: self.deadline,
        })
    }
}

/// Arguments for the Cancel typed data.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub struct BuildEvmProxyCancelTypedDataArgs {
    /// Signed Cancel fields.
    #[serde(flatten)]
    intent: EvmProxyCancelArgs,
    /// Store bound to the deployment.
    store: StringPubkey,
    /// Genesis hash of the Solana cluster, 32 bytes in hex.
    genesis_hash: String,
}

/// EIP-712 typed data for an EVM proxy intent, in the shape viem
/// `signTypedData` and `hashTypedData` take.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi, hashmap_as_object, large_number_types_as_bigints)]
#[serde(rename_all = "camelCase")]
pub struct EvmProxyTypedData {
    domain: EvmProxyTypedDataDomain,
    types: BTreeMap<&'static str, Vec<TypedDataField>>,
    primary_type: &'static str,
    message: BTreeMap<&'static str, TypedDataValue>,
}

/// EIP-712 domain of the typed data.
#[derive(Serialize, Tsify)]
#[tsify(large_number_types_as_bigints)]
#[serde(rename_all = "camelCase")]
pub struct EvmProxyTypedDataDomain {
    name: &'static str,
    version: &'static str,
    chain_id: u64,
    salt: String,
}

/// One member of an EIP-712 type.
#[derive(Serialize, Tsify)]
pub struct TypedDataField {
    name: &'static str,
    #[serde(rename = "type")]
    ty: &'static str,
}

/// One EIP-712 message value.
#[derive(Serialize, Tsify)]
#[tsify(large_number_types_as_bigints)]
#[serde(untagged)]
pub enum TypedDataValue {
    /// `bytes32` in hex.
    Bytes32(String),
    /// `uint64`.
    Uint64(u64),
}

impl EvmProxyTypedData {
    /// `values` follow the member order of `I::TYPE`.
    fn new<I: Intent>(
        program: &Pubkey,
        store: &Pubkey,
        genesis_hash: [u8; 32],
        chain_id: u64,
        values: impl IntoIterator<Item = TypedDataValue>,
    ) -> crate::Result<Self> {
        let invalid_type = || crate::Error::custom("invalid EIP-712 type");
        let (domain_type, domain_members) = type_members(DOMAIN_TYPE).ok_or_else(invalid_type)?;
        let (primary_type, members) = type_members(I::TYPE).ok_or_else(invalid_type)?;
        let members = members.collect::<Vec<_>>();
        let fields = |members: &[(&'static str, &'static str)]| {
            members
                .iter()
                .map(|&(name, ty)| TypedDataField { name, ty })
                .collect()
        };
        let domain = Domain {
            program: *program,
            store: *store,
            genesis_hash,
            chain_id: u64_word(chain_id),
        };
        Ok(Self {
            domain: EvmProxyTypedDataDomain {
                name: DOMAIN_NAME,
                version: DOMAIN_VERSION,
                chain_id,
                salt: hex_string(domain.salt()),
            },
            types: BTreeMap::from([
                (domain_type, fields(&domain_members.collect::<Vec<_>>())),
                (primary_type, fields(&members)),
            ]),
            primary_type,
            message: members.iter().map(|&(name, _)| name).zip(values).collect(),
        })
    }
}

/// Build the EIP-712 typed data of a Cancel for the EVM owner to sign.
#[wasm_bindgen]
pub fn build_evm_proxy_cancel_typed_data(
    args: BuildEvmProxyCancelTypedDataArgs,
) -> crate::Result<EvmProxyTypedData> {
    let program = args.intent.program();
    let intent = args.intent.intent(&program)?;
    EvmProxyTypedData::new::<Cancel>(
        &program.id,
        &args.store,
        parse_hex(&args.genesis_hash, "genesis_hash")?,
        args.intent.chain_id,
        [
            TypedDataValue::Bytes32(hex_string(intent.proxy_account)),
            TypedDataValue::Uint64(intent.nonce),
            TypedDataValue::Uint64(intent.deadline),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
