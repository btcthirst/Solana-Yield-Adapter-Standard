use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::state::DriftAdapterPosition;

/// Returns the current USDC value of the position via set_return_data.
/// Reports the tracked deposited_amount (conservative — does not include accrued interest).
/// Call via simulateTransaction — does not modify state.
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [DriftAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, DriftAdapterPosition>,
}

pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let value = ctx.accounts.adapter_position.deposited_amount;
    set_return_data(&value.to_le_bytes());
    Ok(())
}
