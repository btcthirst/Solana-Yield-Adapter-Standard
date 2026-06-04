use anchor_lang::prelude::*;

pub mod cpi;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("BYT5wbAodWevNJRLnaU2Qe87prHWqycBoZh3oWnCXeY8");

#[program]
pub mod drift_adapter {
    use super::*;

    /// Initialize the adapter position and create Drift IF accounts.
    /// CPIs: initialize_user_stats (if needed) + initialize_insurance_fund_stake.
    /// Must be called once per user before the first deposit.
    /// Also call `dispatcher::initialize_position(adapter_program_id)` separately.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Stake USDC into the Drift Insurance Fund.
    /// CPIs: add_insurance_fund_stake.
    /// Returns `if_shares_received` (u64 LE) via set_return_data for the Dispatcher.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw from the Drift Insurance Fund (2-step cooldown state machine).
    /// First call: request_remove_insurance_fund_stake → starts cooldown (~13 days).
    /// Second call (after cooldown): remove_insurance_fund_stake → transfers USDC back.
    /// Returns `if_shares_removed` (u64 LE) via set_return_data (second call only).
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Returns the current USDC lamport value of the position via set_return_data.
    /// Formula: if_shares * vault_balance / total_shares.
    /// Call via simulateTransaction — does not modify state.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
