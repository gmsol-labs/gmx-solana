use anchor_lang::prelude::*;

/// The Chainlink Program.
pub struct Chainlink;

/// The Chainlink program ID.
///
/// A plain constant on purpose: Anchor's `declare_id!` would also make this the
/// store program's own address in the generated IDL.
pub const ID: Pubkey = pubkey!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");

impl Id for Chainlink {
    fn id() -> Pubkey {
        ID
    }
}
