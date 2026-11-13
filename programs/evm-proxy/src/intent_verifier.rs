use anchor_lang::{
    prelude::*,
    solana_program::{keccak, secp256k1_recover::secp256k1_recover},
};
use gmsol_evm_proxy_intent::{domain_separator, intent_digest};

use crate::{Config, ProxyError, ID};

const EXECUTION_WINDOW: u64 = 3600;
const HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

pub(crate) fn cancel_struct_hash(proxy: &Pubkey, nonce: u64, deadline: u64) -> [u8; 32] {
    gmsol_evm_proxy_intent::cancel_struct_hash(proxy, nonce, deadline)
}

/// The caller supplies a struct hash computed from its own signed fields.
pub(crate) fn verify(
    config: &Config,
    expected_address: [u8; 20],
    chain_id: [u8; 32],
    deadline: u64,
    struct_hash: [u8; 32],
    signature: [u8; 64],
    recovery_id: u8,
) -> Result<()> {
    require!(!config.paused, ProxyError::Paused);
    require!(
        config.allowed_chain_ids.contains(&chain_id),
        ProxyError::ChainNotAllowed
    );
    let now = u64::try_from(Clock::get()?.unix_timestamp)
        .map_err(|_| error!(ProxyError::InvalidDeadline))?;
    let latest = now
        .checked_add(EXECUTION_WINDOW)
        .ok_or(error!(ProxyError::InvalidDeadline))?;
    require!(
        (now..=latest).contains(&deadline),
        ProxyError::InvalidDeadline
    );
    require!(recovery_id <= 1, ProxyError::InvalidSignature);
    let s: [u8; 32] = signature[32..]
        .try_into()
        .map_err(|_| error!(ProxyError::InvalidSignature))?;
    require!(
        s != [0; 32] && s <= HALF_ORDER,
        ProxyError::InvalidSignature
    );

    let domain = domain_separator(&ID, &config.store, &config.genesis_hash, &chain_id);
    let digest = intent_digest(&domain, &struct_hash);
    let public_key = secp256k1_recover(&digest, recovery_id, &signature)
        .map_err(|_| error!(ProxyError::InvalidSignature))?;
    let address_hash = keccak::hash(public_key.to_bytes().as_ref());
    require!(
        address_hash.as_ref()[12..] == expected_address,
        ProxyError::InvalidSignature
    );
    Ok(())
}
