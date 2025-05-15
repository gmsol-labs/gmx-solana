use solana_sdk::pubkey::Pubkey;

mod chainlink {
    solana_sdk::declare_id!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");
}

/// Chainlink Program ID.
pub const CHAINLINK: Pubkey = chainlink::ID;

mod wormhole {
    solana_sdk::declare_id!("HDwcJBJXjL9FpJ7UBsYBtaDjsBUhuLCUYoz3zr8SWWaQ");
}

/// Wormhole Core Bridge Program ID.
pub const WORMHOLE_PROGRAM_ID: Pubkey = wormhole::ID;
