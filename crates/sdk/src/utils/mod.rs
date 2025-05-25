/// Workaround for deserializing ZeroCopy accounts.
pub mod zero_copy;

/// Fixed number convertions.
pub mod fixed;

/// Serialization utils.
pub mod serde;

/// Base64 utils.
pub mod base64;

/// Optional account utils.
pub mod optional;

/// Fixed str.
pub mod fixed_str;

/// Instruction serialization.
pub mod instruction_serialization;

/// Utils for `gmsol-decode`.
#[cfg(feature = "decode")]
pub mod decode;

/// Test utils.
#[cfg(test)]
pub mod test;

pub use fixed::*;
