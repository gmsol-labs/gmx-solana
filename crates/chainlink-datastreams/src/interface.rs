use anchor_lang::{prelude::Pubkey, Ids};

pub use gmsol_mock_chainlink_verifier::{
    cpi::{accounts::VerifyContext, verify},
    DEFAULT_VERIFIER_ACCOUNT_SEEDS as VERIFIER_ACCOUNT_SEEDS,
};

/// Chainlink DataStreams Interface.
#[derive(Clone, Copy)]
pub struct ChainlinkDataStreamsInterface;

impl Ids for ChainlinkDataStreamsInterface {
    fn ids() -> &'static [anchor_lang::prelude::Pubkey] {
        #[cfg(feature = "mock")]
        static IDS: &[Pubkey] = &[crate::mock::ID, crate::verifier::ID];

        #[cfg(not(feature = "mock"))]
        static IDS: &[Pubkey] = &[crate::verifier::ID];

        IDS
    }
}
