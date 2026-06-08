use anchor_lang::prelude::*;

/// Adapter-side position for one (owner, JLP) slot.
///
/// Seeds: [b"jlp_pos", owner]
#[account]
pub struct JupiterLpAdapterPosition {
    /// The wallet that owns this position.
    pub owner: Pubkey,
    /// JLP token balance held by the authority PDA.
    /// 1 JLP = 10^6 lamports (6 decimals, same as USDC).
    pub shares: u64,
    /// Bump for the [b"jlp_auth", owner] authority signer PDA.
    pub authority_bump: u8,
    /// Bump for this account's own PDA.
    pub bump: u8,
}

impl JupiterLpAdapterPosition {
    pub const SPACE: usize = 8 + 32 + 8 + 1 + 1; // 50 bytes
    pub const SEED: &'static [u8] = b"jlp_pos";
    pub const AUTH_SEED: &'static [u8] = b"jlp_auth";
}
