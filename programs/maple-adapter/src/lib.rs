use anchor_lang::prelude::*;

pub mod cpi;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("EuffaJ2ccu1PnppDd5rTBxPvFXA4u8YQKDj6DyqsyVot");

#[program]
pub mod maple_adapter {
    use super::*;

    /// Initialize the adapter position and create the syrupUSDC custody ATA.
    /// Call once per user before the first deposit.
    /// Also call `dispatcher::initialize_position(adapter_program_id)` separately.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Deposit syrupUSDC into the adapter's custody.
    /// `amount` = syrupUSDC lamports (6 decimals).
    ///
    /// syrupUSDC is acquired externally via:
    ///   - Chainlink CCIP bridge from Ethereum syrupUSDC pool
    ///   - Orca/Jupiter swap: USDC → syrupUSDC
    ///
    /// Returns `shares_received` (== `amount`) via set_return_data.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw syrupUSDC from custody back to the user.
    /// `shares == 0` withdraws the entire position.
    ///
    /// Returns `shares_removed` via set_return_data.
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Returns current USDC lamport value of the position via set_return_data.
    /// Reads NAV from the Maple pool state account (updated by Chainlink CCIP).
    /// Intended for `simulateTransaction` — does not modify state.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
