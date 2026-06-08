use anchor_lang::prelude::*;

/// Adapter-side position for one (owner, syrupUSDC) slot.
///
/// Seeds: [b"maple_pos", owner]
#[account]
pub struct MapleAdapterPosition {
    /// Wallet that owns this position.
    pub owner: Pubkey,
    /// syrupUSDC held in authority's ATA (6-decimal lamports).
    pub shares: u64,
    /// Bump for [b"maple_auth", owner] PDA.
    pub authority_bump: u8,
    /// Bump for this PDA.
    pub bump: u8,
}

impl MapleAdapterPosition {
    pub const SPACE: usize = 8 + 32 + 8 + 1 + 1; // 50 bytes
    pub const SEED: &'static [u8] = b"maple_pos";
    pub const AUTH_SEED: &'static [u8] = b"maple_auth";
}
