use anchor_lang::prelude::*;

pub mod cpi;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz");

#[program]
pub mod marginfi_adapter {
    use super::*;

    /// Initialize the adapter position and corresponding MarginFi account.
    /// Call once per user before the first deposit.
    /// Also call `dispatcher::initialize_position(adapter_program_id)` separately.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Deposit USDC into MarginFi via this adapter.
    /// Returns `shares_received` (u64 LE) via set_return_data for the Dispatcher.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw USDC from MarginFi. `shares == 0` withdraws the entire position.
    /// Returns `shares_removed` (u64 LE) via set_return_data for the Dispatcher.
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Returns current USDC lamport value via set_return_data.
    /// Intended for `simulateTransaction` — does not modify state.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
