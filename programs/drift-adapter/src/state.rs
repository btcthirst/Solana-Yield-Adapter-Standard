use anchor_lang::prelude::*;

/// Adapter-side state for a user's Drift spot-market USDC lending position.
/// Tracks deposited amount for current_value and withdrawal.
/// seeds: [b"drift_pos", owner.key()]
#[account]
pub struct DriftAdapterPosition {
    pub owner: Pubkey,           // 32
    pub deposited_amount: u64,   // 8  — USDC atoms deposited into Drift spot market
    pub bump: u8,                // 1
}

impl DriftAdapterPosition {
    pub const SEED: &'static [u8] = b"drift_pos";
    pub const SPACE: usize = 8 + 32 + 8 + 1; // 49
}
