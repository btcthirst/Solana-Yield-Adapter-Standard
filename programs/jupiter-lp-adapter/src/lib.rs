use anchor_lang::prelude::*;

pub mod cpi;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("7JVMN1WEVmXGFdAu5AQsGFfxEAjoL2uD79hEzeo9115E");

#[program]
pub mod jupiter_lp_adapter {
    use super::*;

    /// Initialize the adapter position and create authority token accounts.
    /// Creates two ATAs under the jlp_authority PDA: one for JLP and one for USDC.
    /// Must be called once per user before the first deposit.
    /// Also call `dispatcher::initialize_position(adapter_program_id)` separately.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Deposit USDC into the Jupiter Perpetuals LP pool via this adapter.
    ///
    /// Flow:
    ///   1. Transfer USDC from user's ATA → authority's staging USDC ATA.
    ///   2. CPI `addLiquidity` → JLP minted into authority's JLP ATA.
    ///   3. Record JLP delta as shares.
    ///
    /// Returns `jlp_received` (u64 LE) via set_return_data for the Dispatcher.
    /// Remaining accounts: [0] custody_oracle (USDC oracle from custody.oracle field).
    /// Client must prepend ComputeBudgetProgram.setComputeUnitLimit (≥ 400_000).
    pub fn deposit<'info>(ctx: Context<'info, Deposit<'info>>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw USDC from the Jupiter Perpetuals LP pool. `shares == 0` burns all JLP.
    ///
    /// Flow: CPI `removeLiquidity` burns JLP from authority ATA, sends USDC to user ATA.
    ///
    /// Returns `jlp_burned` (u64 LE) via set_return_data for the Dispatcher.
    /// Remaining accounts: [0] custody_oracle (USDC oracle from custody.oracle field).
    /// Client must prepend ComputeBudgetProgram.setComputeUnitLimit (≥ 400_000).
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Returns current USDC lamport value of the position via set_return_data.
    ///
    /// value = shares * pool.aum_usd / jlp_total_supply
    ///
    /// Call via simulateTransaction — does not modify state.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
