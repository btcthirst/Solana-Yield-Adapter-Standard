use anchor_lang::prelude::*;
use crate::state::UserPosition;
use crate::error::DispatcherError;

#[derive(Accounts)]
#[instruction(adapter: Pubkey)]
pub struct ClosePosition<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        seeds = [UserPosition::SEED, owner.key().as_ref(), adapter.as_ref()],
        bump = user_position.bump,
        constraint = user_position.owner == owner.key() @ DispatcherError::AdapterNotRegistered,
        constraint = user_position.shares == 0 @ DispatcherError::InsufficientShares,
        close = owner,
    )]
    pub user_position: Account<'info, UserPosition>,

    pub system_program: Program<'info, System>,
}

pub fn handler(_ctx: Context<ClosePosition>, _adapter: Pubkey) -> Result<()> {
    // Anchor's `close = owner` handles the lamport transfer and account closure
    Ok(())
}
