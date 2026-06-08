use anchor_lang::prelude::*;

/// Adapter-side position for one (owner, protocol) slot.
///
/// Seeds: [SEED, owner]  @ this program
/// One per user. Stores share count and signer PDA bump.
#[account]
pub struct TemplateAdapterPosition {
    /// Wallet that owns this position.
    pub owner: Pubkey,
    /// Internal share count. Unit is protocol-specific.
    pub shares: u64,
    /// Bump for the [AUTH_SEED, owner] signer PDA.
    pub authority_bump: u8,
    /// Bump for this account's own PDA.
    pub bump: u8,
}

impl TemplateAdapterPosition {
    pub const SPACE: usize = 8 + 32 + 8 + 1 + 1; // 50 bytes
    pub const SEED: &'static [u8] = b"tmpl_pos";
    /// Authority PDA that signs sub-CPIs on behalf of the user.
    pub const AUTH_SEED: &'static [u8] = b"tmpl_auth";
}
