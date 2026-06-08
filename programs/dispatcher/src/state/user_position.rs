use anchor_lang::prelude::*;

#[account]
pub struct UserPosition {
    /// Owner of this position (the user's wallet)
    pub owner: Pubkey,
    /// Adapter program ID this position belongs to
    pub adapter: Pubkey,
    /// Shares held in the protocol (source of truth for the dispatcher layer)
    pub shares: u64,
    pub bump: u8,
}

impl UserPosition {
    // 8 + 32 + 32 + 8 + 1 = 81 bytes
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 1;
    pub const SEED: &'static [u8] = b"position";
}
