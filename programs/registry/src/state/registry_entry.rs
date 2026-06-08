use anchor_lang::prelude::*;

#[account]
pub struct RegistryEntry {
    /// Program ID of the adapter
    pub adapter_program_id: Pubkey,
    /// Human-readable adapter name (null-padded UTF-8)
    pub name: [u8; 64],
    /// Protocol name (null-padded UTF-8)
    pub protocol: [u8; 64],
    /// Base asset mint (e.g. USDC)
    pub asset_mint: Pubkey,
    /// Version, incremented on every update_adapter call
    pub version: u16,
    /// false = permanently revoked; use pause for temporary disable
    pub is_active: bool,
    /// true = temporarily paused (e.g. during exploit response)
    pub is_paused: bool,
    /// Unix timestamp of approval
    pub approved_at: i64,
    /// Authority that approved this adapter
    pub approved_by: Pubkey,
    pub bump: u8,
}

impl RegistryEntry {
    // 8 + 32 + 64 + 64 + 32 + 2 + 1 + 1 + 8 + 32 + 1 = 245 bytes
    pub const SPACE: usize = 8 + 32 + 64 + 64 + 32 + 2 + 1 + 1 + 8 + 32 + 1;
    pub const SEED: &'static [u8] = b"registry_entry";
}
