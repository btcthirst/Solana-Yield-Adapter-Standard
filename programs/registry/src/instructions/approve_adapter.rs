use anchor_lang::prelude::*;
use crate::state::{RegistryState, RegistryEntry};
use crate::error::RegistryError;

#[derive(Accounts)]
#[instruction(adapter_program_id: Pubkey)]
pub struct ApproveAdapter<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [RegistryState::SEED],
        bump = registry_state.bump,
        constraint = registry_state.authority == authority.key() @ RegistryError::Unauthorized,
    )]
    pub registry_state: Account<'info, RegistryState>,

    #[account(
        init,
        payer = authority,
        space = RegistryEntry::SPACE,
        seeds = [RegistryEntry::SEED, adapter_program_id.as_ref()],
        bump,
    )]
    pub registry_entry: Account<'info, RegistryEntry>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<ApproveAdapter>,
    adapter_program_id: Pubkey,
    name: [u8; 64],
    protocol: [u8; 64],
    asset_mint: Pubkey,
) -> Result<()> {
    let clock = Clock::get()?;
    let entry = &mut ctx.accounts.registry_entry;

    entry.adapter_program_id = adapter_program_id;
    entry.name = name;
    entry.protocol = protocol;
    entry.asset_mint = asset_mint;
    entry.version = 1;
    entry.is_active = true;
    entry.is_paused = false;
    entry.approved_at = clock.unix_timestamp;
    entry.approved_by = ctx.accounts.authority.key();
    entry.bump = ctx.bumps.registry_entry;

    emit!(AdapterApproved {
        adapter_program_id,
        protocol: String::from_utf8_lossy(&protocol)
            .trim_end_matches('\0')
            .to_string(),
        approved_by: ctx.accounts.authority.key(),
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

#[event]
pub struct AdapterApproved {
    pub adapter_program_id: Pubkey,
    pub protocol: String,
    pub approved_by: Pubkey,
    pub timestamp: i64,
}
