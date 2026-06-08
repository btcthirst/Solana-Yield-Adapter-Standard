use anchor_lang::prelude::*;
use crate::state::RegistryState;
use crate::error::RegistryError;

#[derive(Accounts)]
pub struct AcceptAuthority<'info> {
    pub new_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [RegistryState::SEED],
        bump = registry_state.bump,
        constraint = registry_state.pending_authority == new_authority.key() @ RegistryError::NotPendingAuthority,
        constraint = registry_state.pending_authority != Pubkey::default() @ RegistryError::NoPendingTransfer,
    )]
    pub registry_state: Account<'info, RegistryState>,
}

pub fn handler(ctx: Context<AcceptAuthority>) -> Result<()> {
    let state = &mut ctx.accounts.registry_state;
    state.authority = state.pending_authority;
    state.pending_authority = Pubkey::default();
    Ok(())
}
