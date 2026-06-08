use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{cpi, error::AdapterError, state::MapleAdapterPosition};

/// Accounts for `current_value`.
///
/// Read-only view instruction — no state changes. Intended for `simulateTransaction`.
/// Prices the custodied syrupUSDC against the live Orca whirlpool (the real
/// secondary-market price ≈ syrupUSDC NAV). There is no native Maple NAV oracle on
/// Solana, so the whirlpool price is the authoritative on-chain value source.
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: position owner; used for PDA seed derivation only.
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [MapleAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MapleAdapterPosition>,

    /// Orca whirlpool (syrupUSDC/USDC) — read for the current price.
    /// CHECK: pinned to the canonical whirlpool; sqrt_price parsed by cpi::read_sqrt_price.
    #[account(address = cpi::SYRUP_WHIRLPOOL @ AdapterError::InvalidPoolState)]
    pub whirlpool: UncheckedAccount<'info>,
}

/// Returns the current USDC lamport value of the position via `set_return_data`.
/// value = shares (syrupUSDC) × whirlpool price (USDC per syrupUSDC).
pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let shares = ctx.accounts.adapter_position.shares;

    if shares == 0 {
        set_return_data(&0u64.to_le_bytes());
        return Ok(());
    }

    let sqrt_price = cpi::read_sqrt_price(&ctx.accounts.whirlpool)?;
    let value = cpi::syrup_to_usdc(shares, sqrt_price).ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
