use anchor_lang::prelude::*;
use registry::state::RegistryEntry;
use crate::state::UserPosition;
use crate::error::DispatcherError;

#[derive(Accounts)]
#[instruction(adapter: Pubkey)]
pub struct InitializePosition<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        init,
        payer = owner,
        space = UserPosition::SPACE,
        seeds = [UserPosition::SEED, owner.key().as_ref(), adapter.as_ref()],
        bump,
    )]
    pub user_position: Account<'info, UserPosition>,

    /// Validated via seeds::program — must be an active, non-paused adapter
    #[account(
        seeds = [RegistryEntry::SEED, adapter.as_ref()],
        bump = registry_entry.bump,
        seeds::program = registry::ID,
        constraint = registry_entry.is_active @ DispatcherError::AdapterRevoked,
        constraint = !registry_entry.is_paused @ DispatcherError::AdapterPaused,
    )]
    pub registry_entry: Account<'info, RegistryEntry>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>, adapter: Pubkey) -> Result<()> {
    let pos = &mut ctx.accounts.user_position;
    pos.owner = ctx.accounts.owner.key();
    pos.adapter = adapter;
    pos.shares = 0;
    pos.bump = ctx.bumps.user_position;
    Ok(())
}
