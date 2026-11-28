use anchor_lang::{prelude::Pubkey, solana_program::keccak};

use crate::{u64_word, Intent};

/// Consume the proxy's next nonce without other effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancel {
    /// Proxy account whose nonce is consumed.
    pub proxy_account: Pubkey,
    /// Proxy nonce consumed by the intent.
    pub nonce: u64,
    /// Latest Unix timestamp at which the intent executes.
    pub deadline: u64,
}

impl Intent for Cancel {
    const TYPE: &'static str = "Cancel(bytes32 proxyAccount,uint64 nonce,uint64 deadline)";

    fn struct_hash(&self) -> [u8; 32] {
        keccak::hashv(&[
            keccak::hash(Self::TYPE.as_bytes()).as_ref(),
            self.proxy_account.as_ref(),
            &u64_word(self.nonce),
            &u64_word(self.deadline),
        ])
        .to_bytes()
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }

    fn deadline(&self) -> u64 {
        self.deadline
    }
}
