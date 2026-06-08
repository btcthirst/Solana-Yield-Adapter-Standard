use anchor_lang::prelude::*;
use crate::state::{RegistryState, RegistryEntry};
use crate::error::RegistryError;

/// Closes a revoked RegistryEntry PDA so the same adapter_program_id can be
/// re-approved later without a PDA collision.
#[derive(Accounts)]
#[instruction(adapter_program_id: Pubkey)]
pub struct CloseRegistryEntry<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [RegistryState::SEED],
        bump = registry_state.bump,
        constraint = registry_state.authority == authority.key() @ RegistryError::Unauthorized,
    )]
    pub registry_state: Account<'info, RegistryState>,

    #[account(
        mut,
        seeds = [RegistryEntry::SEED, adapter_program_id.as_ref()],
        bump = registry_entry.bump,
        constraint = !registry_entry.is_active @ RegistryError::EntryStillActive,
        close = authority,
    )]
    pub registry_entry: Account<'info, RegistryEntry>,
}

pub fn handler(_ctx: Context<CloseRegistryEntry>, _adapter_program_id: Pubkey) -> Result<()> {
    // Anchor's `close = authority` closes the account and transfers rent automatically
    Ok(())
}
