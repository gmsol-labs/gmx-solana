use solana_sdk::signer::Signer;

/// Boxed Signer.
pub type BoxSigner = Box<dyn Signer>;
