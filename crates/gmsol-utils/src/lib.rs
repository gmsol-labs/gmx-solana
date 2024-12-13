/// Utils for price representation.
pub mod price;

/// Fixed-size zero copy map.
pub mod fixed_map;

/// Init space.
pub mod init_space;

/// Zero-copy flags.
pub mod flags;

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
