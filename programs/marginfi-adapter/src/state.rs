use anchor_lang::prelude::*;

/// Adapter-side position for one (owner, USDC MarginFi) slot.
///
/// Seeds: [b"mfi_pos", owner]
/// Separate from Dispatcher's UserPosition — each program owns its own state.
#[account]
pub struct MarginfiAdapterPosition {
    /// The wallet that owns this position.
    pub owner: Pubkey,
    /// Address of the MarginfiAccount PDA created inside the MarginFi program.
    pub marginfi_account: Pubkey,
    /// Internal share count, kept in sync with Dispatcher's UserPosition.shares.
    /// 1 share unit ≈ 1 USDC lamport × ONE_I80F48 / asset_share_value_at_deposit.
    pub shares: u64,
    /// Bump for the [b"mfi_auth", owner] signer PDA.
    pub authority_bump: u8,
    /// Bump for this account's own PDA.
    pub bump: u8,
}

impl MarginfiAdapterPosition {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 1 + 1; // 82 bytes
    pub const SEED: &'static [u8] = b"mfi_pos";
    pub const AUTH_SEED: &'static [u8] = b"mfi_auth";
}
