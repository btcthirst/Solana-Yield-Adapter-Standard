use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, KLEND_PROGRAM_ID, MAIN_LENDING_MARKET},
    error::AdapterError,
    state::KaminoAdapterPosition,
};

/// Read-only instruction — call via simulateTransaction.
/// Returns the current USDC lamport value of the position via set_return_data.
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [KaminoAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, KaminoAdapterPosition>,

    /// CHECK: Kamino lending market; validated to be main market
    #[account(address = MAIN_LENDING_MARKET)]
    pub lending_market: UncheckedAccount<'info>,

    /// CHECK: USDC Reserve; validated via adapter_position
    #[account(
        constraint = reserve.key() == adapter_position.reserve @ AdapterError::InvalidReserve,
    )]
    pub reserve: UncheckedAccount<'info>,

    /// CHECK: must equal KLEND_PROGRAM_ID
    #[account(address = KLEND_PROGRAM_ID)]
    pub klend_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let ctokens = ctx.accounts.adapter_position.shares;

    if ctokens == 0 {
        set_return_data(&0u64.to_le_bytes());
        return Ok(());
    }

    let (total_liquidity, ctoken_supply) = {
        let data = ctx.accounts.reserve.try_borrow_data()?;
        cpi::read_reserve_exchange_data(&data)?
    };

    let value = cpi::ctokens_to_amount(ctokens, total_liquidity, ctoken_supply)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
