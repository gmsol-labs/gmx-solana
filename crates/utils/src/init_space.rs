use anchor_lang::solana_program::pubkey::Pubkey;

/// Alias of [`Space`](anchor_lang::Space).
pub trait InitSpace {
    /// Init Space.
    const INIT_SPACE: usize;
}

impl InitSpace for bool {
    const INIT_SPACE: usize = 1;
}

impl InitSpace for u8 {
    const INIT_SPACE: usize = 1;
}

impl InitSpace for u64 {
    const INIT_SPACE: usize = 8;
}

impl InitSpace for i64 {
    const INIT_SPACE: usize = 8;
}

impl InitSpace for u128 {
    const INIT_SPACE: usize = 16;
}

impl InitSpace for i128 {
    const INIT_SPACE: usize = 16;
}

impl InitSpace for Pubkey {
    const INIT_SPACE: usize = 32;
}

impl<T, const LEN: usize> InitSpace for [T; LEN]
where
    T: InitSpace,
{
    const INIT_SPACE: usize = T::INIT_SPACE * LEN;
}

impl<T> InitSpace for Option<T>
where
    T: InitSpace,
{
    const INIT_SPACE: usize = 1 + T::INIT_SPACE;
}
