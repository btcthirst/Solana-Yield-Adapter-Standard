use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, USDC_BANK},
    error::AdapterError,
    state::MarginfiAdapterPosition,
};

/// Accounts for `current_value`.
///
/// Read-only view instruction — no state changes.
/// Meant to be called via `simulateTransaction`.
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [MarginfiAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MarginfiAdapterPosition>,

    /// CHECK: USDC Bank; read for current asset_share_value
    #[account(constraint = bank.key() == USDC_BANK @ AdapterError::ProtocolError)]
    pub bank: UncheckedAccount<'info>,
}

/// Returns current USDC lamport value of the position via set_return_data.
/// value = shares * asset_share_value / ONE_I80F48
pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let shares = ctx.accounts.adapter_position.shares;

    if shares == 0 {
        set_return_data(&0u64.to_le_bytes());
        return Ok(());
    }

    let asset_share_value = cpi::read_asset_share_value(&ctx.accounts.bank)?;
    let value = cpi::shares_to_amount(shares, asset_share_value)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
