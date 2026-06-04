use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, USDC_IF_VAULT, USDC_SPOT_MARKET},
    error::AdapterError,
    state::DriftAdapterPosition,
};

/// Read-only instruction — call via simulateTransaction.
///
/// Returns the current USDC lamport value of the IF position via set_return_data.
/// Formula: if_shares * vault_balance / total_shares
/// All arithmetic in u128; result cast to u64 (USDC lamports).
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [DriftAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, DriftAdapterPosition>,

    /// CHECK: USDC spot market; contains total_shares in insurance_fund sub-struct
    #[account(address = USDC_SPOT_MARKET)]
    pub spot_market: UncheckedAccount<'info>,

    /// CHECK: USDC insurance fund vault (SPL token account); amount = vault balance
    #[account(address = USDC_IF_VAULT)]
    pub insurance_fund_vault: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let pos = &ctx.accounts.adapter_position;

    if pos.if_shares == 0 {
        set_return_data(&0u64.to_le_bytes());
        return Ok(());
    }

    let total_shares = cpi::read_total_shares(&ctx.accounts.spot_market)?;
    require!(total_shares > 0, AdapterError::ProtocolError);

    let vault_balance = cpi::read_token_account_amount(&ctx.accounts.insurance_fund_vault)? as u128;

    let value = pos
        .if_shares
        .checked_mul(vault_balance)
        .and_then(|v| v.checked_div(total_shares))
        .and_then(|v| u64::try_from(v).ok())
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
