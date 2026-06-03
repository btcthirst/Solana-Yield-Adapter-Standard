use anchor_lang::prelude::*;
use crate::state::RegistryState;
use crate::error::RegistryError;

#[derive(Accounts)]
pub struct ProposeAuthority<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [RegistryState::SEED],
        bump = registry_state.bump,
        constraint = registry_state.authority == authority.key() @ RegistryError::Unauthorized,
    )]
    pub registry_state: Account<'info, RegistryState>,
}

pub fn handler(ctx: Context<ProposeAuthority>, new_authority: Pubkey) -> Result<()> {
    ctx.accounts.registry_state.pending_authority = new_authority;
    Ok(())
}
