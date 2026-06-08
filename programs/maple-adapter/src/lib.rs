use anchor_lang::prelude::*;

pub mod cpi;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("EuffaJ2ccu1PnppDd5rTBxPvFXA4u8YQKDj6DyqsyVot");

// Maple (syrupUSDC) yield adapter.
//
// Maple has NO native deposit program on Solana — syrupUSDC is a Chainlink-CCIP
// bridge token whose mint/redeem happens on Ethereum. To still offer the uniform
// USDC-in / USDC-out interface, this adapter routes through the live Orca whirlpool
// (syrupUSDC/USDC): `deposit` swaps USDC -> syrupUSDC and custodies it; `withdraw`
// swaps syrupUSDC -> USDC and pays the user. The yield comes from syrupUSDC's NAV
// appreciation; `current_value` prices the position against the whirlpool. See SPEC.md §3.
#[program]
pub mod maple_adapter {
    use super::*;

    /// Initialize the adapter position and create both custody ATAs
    /// (syrupUSDC + USDC) under the authority PDA. Call once per user before the
    /// first deposit. Also call `dispatcher::initialize_position(adapter_program_id)`.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Deposit `amount` USDC lamports: pull from the user, swap USDC -> syrupUSDC on
    /// the Orca whirlpool, and custody the syrupUSDC.
    /// Returns `shares_received` (syrupUSDC lamports acquired) via set_return_data.
    /// Client must prepend ComputeBudgetProgram.setComputeUnitLimit (≥ 400_000).
    pub fn deposit<'info>(ctx: Context<'info, Deposit<'info>>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw `shares` syrupUSDC lamports: swap to USDC on the Orca whirlpool and
    /// pay the user. `shares == 0` withdraws the entire position.
    /// Returns `shares_removed` via set_return_data.
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Returns current USDC lamport value of the position via set_return_data.
    /// Prices custodied syrupUSDC against the live Orca whirlpool.
    /// Intended for `simulateTransaction` — does not modify state.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
