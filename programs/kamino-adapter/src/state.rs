use anchor_lang::prelude::*;

/// Adapter-side position for one (owner, Kamino USDC Lending) slot.
///
/// Seeds: [b"k_pos", owner]
#[account]
pub struct KaminoAdapterPosition {
    /// The wallet that owns this position.
    pub owner: Pubkey,
    /// Kamino Obligation PDA (owned by kamino_authority, created in initialize_position).
    pub obligation: Pubkey,
    /// USDC Reserve address in the Kamino lending market (set at initialize_position).
    pub reserve: Pubkey,
    /// Collateral token (kUSDC) count held in the obligation.
    /// Tracks ctokens deposited; used as collateral_amount in withdraw CPI.
    pub shares: u64,
    /// Bump for the [b"k_auth", owner] authority signer PDA.
    pub authority_bump: u8,
    /// Bump for this account's own PDA.
    pub bump: u8,
}

impl KaminoAdapterPosition {
    pub const SPACE: usize = 8 + 32 + 32 + 32 + 8 + 1 + 1; // 114 bytes
    pub const SEED: &'static [u8] = b"k_pos";
    pub const AUTH_SEED: &'static [u8] = b"k_auth";
}
