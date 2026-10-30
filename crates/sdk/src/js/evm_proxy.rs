use std::str::FromStr;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

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
}
