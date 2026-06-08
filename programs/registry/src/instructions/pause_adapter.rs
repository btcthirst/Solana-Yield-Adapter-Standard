use anchor_lang::prelude::*;
use crate::state::{RegistryState, RegistryEntry};
use crate::error::RegistryError;

#[derive(Accounts)]
#[instruction(adapter_program_id: Pubkey)]
pub struct PauseAdapter<'info> {
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
    )]
    pub registry_entry: Account<'info, RegistryEntry>,
}

pub fn handler(ctx: Context<PauseAdapter>, _adapter_program_id: Pubkey) -> Result<()> {
    let clock = Clock::get()?;
    let entry = &mut ctx.accounts.registry_entry;
    entry.is_paused = true;

    emit!(AdapterPaused {
        adapter_program_id: entry.adapter_program_id,
        paused_by: ctx.accounts.authority.key(),
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

#[event]
pub struct AdapterPaused {
    pub adapter_program_id: Pubkey,
    pub paused_by: Pubkey,
    pub timestamp: i64,
}
