use crate::{pda::EvmAddressBytes, serde::StringPubkey};
use serde::{Deserialize, Serialize};
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
