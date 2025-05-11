use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct LeaderEntry {
    pub address: Pubkey,
    pub volume: u64,
}

#[account]
pub struct Competition {
    pub authority: Pubkey,
    pub start_time: i64,
    pub end_time: i64,
    pub is_active: bool,
    pub store_program: Pubkey,

    // New: top-5 leaderboard (fixed-length Vec)
    pub leaderboard: Vec<LeaderEntry>, // length ≤ 5
}
impl Competition {
    pub const LEN: usize = 32 + 8 + 8 + 1 + 32 + 4 + 200;
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
