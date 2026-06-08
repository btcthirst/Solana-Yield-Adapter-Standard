use anchor_lang::prelude::*;

pub mod cpi;
pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

// Placeholder ID — matches the bundled target/deploy/template_adapter-keypair.json
// so a fresh `anchor build` succeeds out of the box. Replace with your own before
// deploying:
//   solana-keygen new --outfile target/deploy/my_adapter-keypair.json
//   anchor keys sync   (updates this declare_id! and Anchor.toml)
declare_id!("6TiEx46An5whVbruUqyMxYJmCmkUJrGPrtBtuM6N7NyR");

#[program]
pub mod template_adapter {
    use super::*;

    /// Initialize the adapter position and any protocol-side accounts.
    /// Called directly by the user before the first deposit.
    pub fn initialize_position(ctx: Context<InitializePosition>) -> Result<()> {
        instructions::initialize_position::handler(ctx)
    }

    /// Deposit `amount` asset lamports into the protocol.
    /// Called by the Dispatcher via invoke() — owner is NOT a Signer here.
    /// Must call set_return_data(&shares_received.to_le_bytes()) before returning.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, amount)
    }

    /// Withdraw `shares` from the protocol. shares == 0 means "withdraw all".
    /// Must call set_return_data(&shares_removed.to_le_bytes()) before returning.
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, shares)
    }

    /// Return current USDC lamport value of the position via set_return_data.
    /// Must be read-only — intended for simulateTransaction.
    pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
        instructions::current_value::handler(ctx)
    }
}
