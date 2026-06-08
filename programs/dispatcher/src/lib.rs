use anchor_lang::prelude::*;

pub mod cpi_utils;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("F6QyZM6rb5i1bDsW9gQMrPBVzZeEfSRbR2JDzDQTJuQ1");

#[program]
pub mod dispatcher {
    use super::*;

    /// Create a UserPosition PDA for the (owner, adapter) pair.
    /// Must be called once before the first deposit.
    pub fn initialize_position(
        ctx: Context<InitializePosition>,
        adapter: Pubkey,
    ) -> Result<()> {
        instructions::initialize_position::handler(ctx, adapter)
    }

    /// Route a deposit to the given adapter.
    /// Adapter must return shares_received via set_return_data.
    /// Client must add ComputeBudgetProgram.setComputeUnitLimit as first instruction.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Route a withdrawal to the given adapter.
    /// shares = 0 means "withdraw all".
    /// Adapter returns shares_removed via set_return_data (may be 0 for cooldown protocols).
    pub fn withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Close an empty UserPosition PDA (shares must be 0) and return rent to owner.
    pub fn close_position(ctx: Context<ClosePosition>, adapter: Pubkey) -> Result<()> {
        instructions::close_position::handler(ctx, adapter)
    }

    /// Query the current USD value of the position.
    /// Call via simulateTransaction — does not submit a transaction or pay fees.
    /// Returns u64 (USDC lamports) via returnData.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
