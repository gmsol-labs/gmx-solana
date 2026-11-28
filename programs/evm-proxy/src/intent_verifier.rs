use anchor_lang::prelude::*;
use gmsol_evm_proxy_intent::{
    digest, is_within_execution_window, recover_signer, ChainId, Domain, Intent,
};

use crate::{load_proxy, Config, Proxy, ProxyError, ID};

/// Consume the proxy's next nonce with a signed intent after checking Config
/// policy, the deadline and the EVM owner's signature.
pub(crate) fn consume(
    config: &Config,
    proxy: &AccountLoader<Proxy>,
    chain_id: ChainId,
    intent: &impl Intent,
    signature: [u8; 64],
    recovery_id: u8,
) -> Result<()> {
    let owner = load_proxy(proxy)?.evm_address;
    require!(!config.paused, ProxyError::Paused);
    require!(
        config.allowed_chain_ids.contains(&chain_id),
        ProxyError::ChainNotAllowed
    );
    require!(
        is_within_execution_window(Clock::get()?.unix_timestamp, intent.deadline()),
        ProxyError::InvalidDeadline
    );
    let domain = Domain {
        program: ID,
        store: config.store,
        genesis_hash: config.genesis_hash,
        chain_id,
    };
    require!(
        recover_signer(&digest(&domain, intent), &signature, recovery_id) == Some(owner),
        ProxyError::InvalidSignature
    );
    proxy.load_mut()?.consume_next_nonce(intent.nonce())
}
