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

    /// Initialize the adapter position and create Drift spot-market accounts.
    /// CPIs: initialize_user_stats (if needed) + initialize_user (sub_account_id=0).
    /// Must be called once per user before the first deposit.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Deposit USDC into the Drift spot market (lending).
    /// CPI: deposit(market_index=0, amount, reduce_only=false).
    /// Returns `amount` (u64 LE) via set_return_data for the Dispatcher.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw USDC from the Drift spot market.
    /// Single-step — no cooldown unlike IF staking.
    /// CPI: withdraw(market_index=0, deposited_amount, reduce_only=false).
    /// Returns `deposited_amount` (u64 LE) via set_return_data.
    pub fn withdraw<'info>(
        ctx: Context<'info, Withdraw<'info>>,
        shares: u64,
    ) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Returns the tracked USDC value of the position via set_return_data.
    /// Call via simulateTransaction — does not modify state.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
