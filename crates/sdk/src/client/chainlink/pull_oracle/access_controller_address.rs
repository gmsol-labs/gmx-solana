use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

pub(super) const DEVNET_ADDRESS: Pubkey = pubkey!("2k3DsgwBoqrnvXKVvd7jX7aptNxdcRBdcd5HkYsGgbrb");
#[cfg(not(feature = "devnet"))]
pub(super) const MAINNET_ADDRESS: Pubkey = pubkey!("7mSn5MoBjyRLKoJShgkep8J17ueGG8rYioVAiSg5YWMF");

#[cfg(feature = "devnet")]
pub(super) const ADDRESS: Pubkey = DEVNET_ADDRESS;

#[cfg(not(feature = "devnet"))]
pub(super) const ADDRESS: Pubkey = MAINNET_ADDRESS;
