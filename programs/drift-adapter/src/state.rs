use anchor_lang::prelude::*;

/// Adapter-side state for a user's Drift Insurance Fund position.
///
/// Tracks full u128 if_shares for precision, plus the 2-step unstake state machine.
/// seeds: [b"drift_pos", owner.key()]
#[account]
pub struct DriftAdapterPosition {
    pub owner: Pubkey,                    // 32
    pub if_shares: u128,                  // 16  — full precision Drift IF shares
    pub market_index: u16,                // 2   — USDC = 0
    pub pending_withdrawal: bool,         // 1   — cooldown in progress
    pub withdrawal_request_shares: u128,  // 16  — shares locked for withdrawal
    pub withdrawal_request_ts: i64,       // 8   — unix ts when request was made
    pub bump: u8,                         // 1
}

impl DriftAdapterPosition {
    pub const SEED: &'static [u8] = b"drift_pos";
    pub const SPACE: usize = 8 + 32 + 16 + 2 + 1 + 16 + 8 + 1; // 84
}
