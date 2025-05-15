/// Utils for price representation.
pub mod price;

/// Fixed-size zero copy map.
pub mod fixed_map;

/// Definition for [`InitSpace`].
pub mod init_space;

/// Zero-copy flags.
pub mod flags;

/// Fixed str.
pub mod fixed_str;

/// Pubkey utils.
pub mod pubkey;

/// Oracle utils.
pub mod oracle;

/// Definitions related to market.
pub mod market;

/// Definitions related to token config.
pub mod token_config;

/// Utils for dynamic access to an array of zero copy types.
pub mod dynamic_access;

/// Definitions related to action.
pub mod action;

/// A `slice::chunk_by` implementation, copied from `std`.
pub mod chunk_by;

/// Swap parameters.
pub mod swap;

/// Definitions related to order.
pub mod order;

/// Definitions related to GLV.
pub mod glv;

/// Definitions related to global configurations.
pub mod config;

/// Utils for GT.
pub mod gt;

/// Convert a string to a seed.
pub fn to_seed(key: &str) -> [u8; 32] {
    use anchor_lang::solana_program::hash::hash;
    hash(key.as_bytes()).to_bytes()
}

pub use self::{init_space::InitSpace, price::Price};
pub use bitmaps;
pub use paste;
pub use static_assertions;

/// General-purpose errors.
#[anchor_lang::error_code]
pub enum GeneralError {
    /// Already Exist.
    #[msg("Already exist")]
    AlreadyExist,
    /// Exceed length limit.
    #[msg("Exceed max length limit")]
    ExceedMaxLengthLimit,
}
