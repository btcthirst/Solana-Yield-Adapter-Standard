use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{cpi, error::AdapterError, state::MapleAdapterPosition};

/// Accounts for `deposit`.
///
/// Called indirectly via the Dispatcher (invoke), so `owner` is not a Signer —
/// the adapter's authority PDA signs the token transfer sub-CPI.
///
/// The caller deposits syrupUSDC directly. syrupUSDC is acquired externally
/// (via Chainlink CCIP from Ethereum, or via Orca/Jupiter swap on Solana).
#[derive(Accounts)]
pub struct Deposit<'info> {
    /// CHECK: position owner; used for PDA seed derivation only
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [MapleAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MapleAdapterPosition>,

    /// Virtual authority PDA — signs token transfers to/from adapter's ATA.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [MapleAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub maple_authority: UncheckedAccount<'info>,

    /// User's syrupUSDC ATA (source of deposit).
    /// CHECK: SPL token account; amount validated by token program on transfer
    #[account(mut)]
    pub user_syrup_ata: UncheckedAccount<'info>,

    /// Authority's syrupUSDC ATA (destination custody).
    /// CHECK: validated as authority's ATA via associated token derivation
    #[account(mut)]
    pub authority_syrup_ata: UncheckedAccount<'info>,

    /// CHECK: must be SPL Token program
    pub token_program: UncheckedAccount<'info>,
}

/// Deposit `amount` syrupUSDC (6-decimal lamports) into the adapter.
///
/// Returns `shares_received` (== `amount`) via `set_return_data` for the Dispatcher.
/// 1 share unit = 1 syrupUSDC lamport.
pub fn handler(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::InsufficientFunds);

    // Transfer syrupUSDC: user_syrup_ata → authority_syrup_ata.
    // Authority for this transfer is the owner (user wallet), whose signer flag
    // propagates through the Dispatcher's invoke chain.
    cpi::cpi_token_transfer(
        &ctx.accounts.token_program,
        &ctx.accounts.user_syrup_ata,
        &ctx.accounts.authority_syrup_ata,
        &ctx.accounts.owner,
        amount,
        &[], // owner signer propagated by invoke — no PDA seeds needed
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos
        .shares
        .checked_add(amount)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&amount.to_le_bytes());
    Ok(())
}
