use anchor_lang::prelude::*;
use crate::state::{RegistryState, RegistryEntry};
use crate::error::RegistryError;

#[derive(Accounts)]
#[instruction(adapter_program_id: Pubkey)]
pub struct UpdateAdapter<'info> {
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

pub fn handler(
    ctx: Context<UpdateAdapter>,
    _adapter_program_id: Pubkey,
    name: Option<[u8; 64]>,
    protocol: Option<[u8; 64]>,
    asset_mint: Option<Pubkey>,
) -> Result<()> {
    let clock = Clock::get()?;
    let entry = &mut ctx.accounts.registry_entry;

    if let Some(n) = name {
        entry.name = n;
    }
    if let Some(p) = protocol {
        entry.protocol = p;
    }
    if let Some(m) = asset_mint {
        entry.asset_mint = m;
    }

    entry.version = entry.version.saturating_add(1);

    emit!(AdapterUpdated {
        adapter_program_id: entry.adapter_program_id,
        new_version: entry.version,
        updated_by: ctx.accounts.authority.key(),
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

#[event]
pub struct AdapterUpdated {
    pub adapter_program_id: Pubkey,
    pub new_version: u16,
    pub updated_by: Pubkey,
    pub timestamp: i64,
}
