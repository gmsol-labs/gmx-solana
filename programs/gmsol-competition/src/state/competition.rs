use anchor_lang::prelude::*;

#[account]
pub struct Competition {
    pub authority: Pubkey,
    pub start_time: i64,
    pub end_time: i64,
    pub is_active: bool,
    pub store_program: Pubkey,
}
impl Competition {
    pub const LEN: usize = 32 + 8 + 8 + 1 + 32;
}

#[account]
pub struct Participant {
    pub competition: Pubkey,
    pub owner: Pubkey,
    pub volume: u64,
    pub last_updated_at: i64,
}
impl Participant {
    pub const LEN: usize = 32 + 32 + 8 + 8;
}