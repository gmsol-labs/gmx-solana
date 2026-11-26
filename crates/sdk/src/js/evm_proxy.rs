use crate::{pda::EvmAddressBytes, serde::StringPubkey};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

const EVM_ADDRESS_LEN: usize = std::mem::size_of::<EvmAddressBytes>();
const EVM_ADDRESS_HEX_LEN: usize = EVM_ADDRESS_LEN * 2;

/// Proxy PDA and its canonical bump.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ProxyAddress {
    /// Base58 proxy account address.
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

#[cfg(test)]
mod tests {
    use super::*;
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
