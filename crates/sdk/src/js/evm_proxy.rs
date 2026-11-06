use std::str::FromStr;

use serde::{Deserialize, Serialize};
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
