use anchor_lang::prelude::*;

#[account]
pub struct RegistryState {
    /// Governance authority (can approve/revoke/pause adapters)
    pub authority: Pubkey,
    /// Pending authority for 2-step transfer (zero if no transfer in progress)
    pub pending_authority: Pubkey,
    pub bump: u8,
}

impl RegistryState {
    pub const SPACE: usize = 8 + 32 + 32 + 1; // 73 bytes
    pub const SEED: &'static [u8] = b"registry_state";
}
