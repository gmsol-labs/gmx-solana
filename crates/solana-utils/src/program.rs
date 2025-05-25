use std::ops::Deref;

use solana_sdk::{pubkey::Pubkey, signer::Signer};

#[cfg(client)]
use solana_client::nonblocking::rpc_client::RpcClient;

use crate::transaction_builder::{Config, TransactionBuilder};

/// Program.
pub struct Program<C> {
    program_id: Pubkey,
    cfg: Config<C>,
}

impl<C> Program<C> {
    /// Create a new [`Program`].
    pub fn new(program_id: Pubkey, cfg: Config<C>) -> Self {
        Self { program_id, cfg }
    }

    /// Create a Solana RPC Client.
    #[cfg(client)]
    pub fn rpc(&self) -> RpcClient {
        self.cfg.rpc()
    }

    /// Get the program id.
    pub fn id(&self) -> &Pubkey {
        &self.program_id
    }
}

impl<C: Deref<Target = impl Signer> + Clone> Program<C> {
    /// Create a [`TransactionBuilder`].
    pub fn transaction<'a>(&self) -> TransactionBuilder<'a, C> {
        TransactionBuilder::new(self.program_id, &self.cfg)
    }
}

impl<C: Deref<Target = impl Signer>> Program<C> {
    /// Get the pubkey of the payer.
    pub fn payer(&self) -> Pubkey {
        self.cfg.payer()
    }
}
