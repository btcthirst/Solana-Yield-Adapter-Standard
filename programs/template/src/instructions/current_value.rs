use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{cpi, error::AdapterError, state::TemplateAdapterPosition};

/// Accounts for `current_value`.
///
/// Read-only — no state mutations. Called via Dispatcher CPI inside simulateTransaction.
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: used for PDA seed derivation only
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [TemplateAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, TemplateAdapterPosition>,

    // TODO: add oracle / market accounts needed to compute the live exchange rate
}

pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let shares = ctx.accounts.adapter_position.shares;

    // TODO: read live exchange rate
    // let rate = cpi::read_exchange_rate(&ctx.accounts.protocol_state)?;
    let rate = 1_000_000u64;

    let value_usdc = cpi::shares_to_amount(shares, rate)
        .ok_or(error!(AdapterError::Overflow))?;

    // REQUIRED: write USDC lamport value to return data.
    set_return_data(&value_usdc.to_le_bytes());
    Ok(())
}
