use anchor_lang::prelude::*;
use crate::state::{RegistryState};

#[derive(Accounts)]
pub struct InitializeRegistry<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = RegistryState::SPACE,
        seeds = [RegistryState::SEED],
        bump,
    )]
    pub registry_state: Account<'info, RegistryState>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializeRegistry>, authority: Pubkey) -> Result<()> {
    let state = &mut ctx.accounts.registry_state;
    state.authority = authority;
    state.pending_authority = Pubkey::default();
    state.bump = ctx.bumps.registry_state;
    Ok(())
}
