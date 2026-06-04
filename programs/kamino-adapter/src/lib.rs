use anchor_lang::prelude::*;

pub mod cpi;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("5ksJ5dU6jAoZaUnpcXtGN69xXewcRcGLTBisQHSkwc44");

#[program]
pub mod kamino_adapter {
    use super::*;

    /// Initialize the adapter position and create a Kamino Obligation.
    /// Calls Kamino's `init_user_metadata` (if needed) and `init_obligation`.
    /// Must be called once per user before the first deposit.
    /// Also call `dispatcher::initialize_position(adapter_program_id)` separately.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Deposit USDC into Kamino Lending via this adapter.
    /// CPIs: refresh_reserve → deposit_reserve_liquidity_and_obligation_collateral.
    /// Returns `ctokens_received` (u64 LE) via set_return_data for the Dispatcher.
    /// Client must prepend ComputeBudgetProgram.setComputeUnitLimit (≥ 400_000).
    pub fn deposit<'info>(ctx: Context<'info, Deposit<'info>>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw USDC from Kamino. `shares == 0` withdraws the entire position.
    /// CPIs: refresh_reserve → refresh_obligation → withdraw_obligation_collateral_and_redeem_reserve_collateral.
    /// Returns `ctokens_removed` (u64 LE) via set_return_data for the Dispatcher.
    /// Client must prepend ComputeBudgetProgram.setComputeUnitLimit (≥ 500_000).
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Returns current USDC lamport value of the position via set_return_data.
    /// Call via simulateTransaction — does not modify state.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
