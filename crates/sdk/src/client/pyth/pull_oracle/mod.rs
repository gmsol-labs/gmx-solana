/// Wormhole Ops.
pub mod wormhole;

/// Pyth Reciever Ops.
pub mod receiver;

/// Hermes.
pub mod hermes;

/// Utils.
pub mod utils;

mod pull_oracle_impl;

use std::ops::Deref;

use gmsol_solana_utils::program::Program;
use solana_sdk::signer::Signer;

use self::wormhole::WORMHOLE_PROGRAM_ID;

pub use self::{
    pull_oracle_impl::{PriceUpdates, PythPullOracleWithHermes},
    receiver::PythReceiverOps,
    wormhole::WormholeOps,
};

const VAA_SPLIT_INDEX: usize = 755;

/// Pyth Pull Oracle.
pub struct PythPullOracle<C> {
    wormhole: Program<C>,
    pyth: Program<C>,
}

impl<S, C> PythPullOracle<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    /// Create a new [`PythPullOracle`] client from [`Client`](crate::Client).
    pub fn try_new(client: &crate::Client<C>) -> crate::Result<Self> {
        Ok(Self {
            wormhole: client.program(WORMHOLE_PROGRAM_ID),
            pyth: client.program(pyth_solana_receiver_sdk::ID),
        })
    }
}
