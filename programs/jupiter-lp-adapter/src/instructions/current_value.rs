use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, JLP_MINT, JLP_POOL},
    error::AdapterError,
    state::JupiterLpAdapterPosition,
};

/// Read-only instruction — call via simulateTransaction.
/// Returns the current USDC lamport value of the position via set_return_data.
///
/// Value = shares * pool.aum_usd / jlp_total_supply
/// All quantities use 6-decimal precision (USDC lamports equivalent).
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [JupiterLpAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, JupiterLpAdapterPosition>,

    /// Jupiter Perpetuals JLP Pool account.
    /// CHECK: validated by address constraint; aum_usd read from raw bytes
    #[account(address = JLP_POOL)]
    pub pool: UncheckedAccount<'info>,

    /// JLP token mint — supply read from raw bytes at offset 36.
    /// CHECK: validated by address constraint
    #[account(address = JLP_MINT)]
    pub jlp_mint: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let shares = ctx.accounts.adapter_position.shares;

    if shares == 0 {
        set_return_data(&0u64.to_le_bytes());
        return Ok(());
    }

    let aum_usd = {
        let data = ctx.accounts.pool.try_borrow_data()?;
        cpi::read_pool_aum(&data)?
    };

    let jlp_supply = {
        let data = ctx.accounts.jlp_mint.try_borrow_data()?;
        cpi::read_mint_supply(&data)?
    };

    let value = cpi::jlp_to_usdc(shares, aum_usd, jlp_supply)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
