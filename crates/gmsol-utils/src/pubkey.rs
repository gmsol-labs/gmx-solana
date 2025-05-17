use anchor_lang::solana_program::pubkey::Pubkey;

/// Convert to bytes with only the reference of a [`Pubkey`].
pub fn to_bytes(pubkey: &Pubkey) -> [u8; 32] {
    pubkey.to_bytes()
}

/// The "default" pubkey.
pub const DEFAULT_PUBKEY: Pubkey = Pubkey::new_from_array([0; 32]);

/// Parse optional address where the default pubkey is treated as `None`.
pub fn optional_address(pubkey: &Pubkey) -> Option<&Pubkey> {
    if *pubkey == DEFAULT_PUBKEY {
        None
    } else {
        Some(pubkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optional_address() {
        assert_eq!(Pubkey::default(), DEFAULT_PUBKEY);
        assert_eq!(optional_address(&DEFAULT_PUBKEY), None);
        assert_eq!(optional_address(&Pubkey::default()), None);
        let address = Pubkey::new_unique();
        assert_eq!(optional_address(&address), Some(&address));
    }
}
