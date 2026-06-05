use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{cpi, error::AdapterError, state::MapleAdapterPosition};

/// Accounts for `current_value`.
///
/// Read-only view instruction — no state changes.
/// Intended for `simulateTransaction`.
#[derive(Accounts)]
pub struct CurrentValue<'info> {
    /// CHECK: position owner; used for PDA seed derivation only
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [MapleAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MapleAdapterPosition>,

    /// Canonical Maple pool state account, pushed from Ethereum via Chainlink CCIP.
    /// Stores total_assets_usdc and total_syrup_supply for NAV computation.
    /// CHECK: read-only; identity pinned by the address constraint, contents
    /// parsed (with bounds checks) by cpi::read_nav.
    #[account(address = cpi::MAPLE_POOL_STATE @ AdapterError::InvalidPoolState)]
    pub pool_state: UncheckedAccount<'info>,
}

/// Returns current USDC lamport value of the position via `set_return_data`.
///
/// value = shares × nav / 1e6
/// where nav = pool_state.total_assets_usdc / pool_state.total_syrup_supply × 1e6.
pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let shares = ctx.accounts.adapter_position.shares;

    if shares == 0 {
        set_return_data(&0u64.to_le_bytes());
        return Ok(());
    }

    let nav = cpi::read_nav(&ctx.accounts.pool_state);
    if nav.is_fallback {
        // Oracle unreadable (CCIP not yet populated, or layout unverified):
        // report the conservative 1.0 floor. Logged so the client can tell a
        // fallback apart from a verified NAV — value may understate accrued yield.
        msg!("maple: NAV oracle unavailable — using conservative 1.0 floor (value may understate yield)");
    }
    let value = cpi::shares_to_usdc(shares, nav.value).ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
