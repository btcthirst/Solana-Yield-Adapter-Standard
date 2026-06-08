use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("4NP3DgbM7JJDBQEiU9ojUJ3yYnCoEnbBCsACfQz32xdB");

#[program]
pub mod registry {
    use super::*;

    /// One-time initialization of the Registry state account.
    pub fn initialize_registry(
        ctx: Context<InitializeRegistry>,
        authority: Pubkey,
    ) -> Result<()> {
        instructions::initialize_registry::handler(ctx, authority)
    }

    /// Approve a new adapter and create its RegistryEntry PDA.
    pub fn approve_adapter(
        ctx: Context<ApproveAdapter>,
        adapter_program_id: Pubkey,
        name: [u8; 64],
        protocol: [u8; 64],
        asset_mint: Pubkey,
    ) -> Result<()> {
        instructions::approve_adapter::handler(ctx, adapter_program_id, name, protocol, asset_mint)
    }

    /// Permanently revoke an adapter (is_active = false).
    pub fn revoke_adapter(
        ctx: Context<RevokeAdapter>,
        adapter_program_id: Pubkey,
    ) -> Result<()> {
        instructions::revoke_adapter::handler(ctx, adapter_program_id)
    }

    /// Temporarily pause an adapter (is_paused = true).
    pub fn pause_adapter(
        ctx: Context<PauseAdapter>,
        adapter_program_id: Pubkey,
    ) -> Result<()> {
        instructions::pause_adapter::handler(ctx, adapter_program_id)
    }

    /// Resume a paused adapter (is_paused = false).
    pub fn resume_adapter(
        ctx: Context<ResumeAdapter>,
        adapter_program_id: Pubkey,
    ) -> Result<()> {
        instructions::resume_adapter::handler(ctx, adapter_program_id)
    }

    /// Update adapter metadata (name, protocol, asset_mint). Bumps version.
    pub fn update_adapter(
        ctx: Context<UpdateAdapter>,
        adapter_program_id: Pubkey,
        name: Option<[u8; 64]>,
        protocol: Option<[u8; 64]>,
        asset_mint: Option<Pubkey>,
    ) -> Result<()> {
        instructions::update_adapter::handler(ctx, adapter_program_id, name, protocol, asset_mint)
    }

    /// Close a revoked RegistryEntry PDA, returning rent to authority.
    /// Required before re-approving the same adapter_program_id.
    pub fn close_registry_entry(
        ctx: Context<CloseRegistryEntry>,
        adapter_program_id: Pubkey,
    ) -> Result<()> {
        instructions::close_registry_entry::handler(ctx, adapter_program_id)
    }

    /// Propose a new authority (step 1 of 2-step transfer).
    pub fn propose_authority(
        ctx: Context<ProposeAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        instructions::propose_authority::handler(ctx, new_authority)
    }

    /// Accept authority transfer (step 2 of 2-step transfer).
    pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
        instructions::accept_authority::handler(ctx)
    }
}
