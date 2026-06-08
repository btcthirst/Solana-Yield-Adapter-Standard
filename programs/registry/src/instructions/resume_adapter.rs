use anchor_lang::prelude::*;
use crate::state::{RegistryState, RegistryEntry};
use crate::error::RegistryError;

#[derive(Accounts)]
#[instruction(adapter_program_id: Pubkey)]
pub struct ResumeAdapter<'info> {
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

pub fn handler(ctx: Context<ResumeAdapter>, _adapter_program_id: Pubkey) -> Result<()> {
    let clock = Clock::get()?;
    let entry = &mut ctx.accounts.registry_entry;
    entry.is_paused = false;

    emit!(AdapterResumed {
        adapter_program_id: entry.adapter_program_id,
        resumed_by: ctx.accounts.authority.key(),
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

#[event]
pub struct AdapterResumed {
    pub adapter_program_id: Pubkey,
    pub resumed_by: Pubkey,
    pub timestamp: i64,
}
