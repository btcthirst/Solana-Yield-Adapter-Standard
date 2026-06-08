use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{cpi, error::AdapterError, state::TemplateAdapterPosition};

/// Accounts for `deposit`.
///
/// Called via Dispatcher CPI (invoke) — `owner` is NOT a Signer here.
/// All accounts are passed as remaining_accounts from the Dispatcher transaction
/// in the exact order they appear in this struct.
#[derive(Accounts)]
pub struct Deposit<'info> {
    /// CHECK: used for PDA seed derivation only
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [TemplateAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, TemplateAdapterPosition>,

    /// CHECK: authority signer PDA; signs sub-CPIs via invoke_signed
    #[account(
        seeds = [TemplateAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub authority: UncheckedAccount<'info>,

    // TODO: add protocol-specific accounts (token accounts, market state, protocol program, ...)
    // Keep the order stable — it must match the remaining_accounts array the client sends.
}

pub fn handler(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::InsufficientFunds);

    // 1. Read exchange rate from protocol state.
    // let rate = cpi::read_exchange_rate(&ctx.accounts.protocol_state)?;
    let rate = 1_000_000u64; // TODO: replace with real read

    // 2. Compute shares.
    let shares = cpi::amount_to_shares(amount, rate)
        .ok_or(error!(AdapterError::Overflow))?;

    // 3. Build signer seeds and execute protocol CPI.
    let owner_key = ctx.accounts.owner.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        TemplateAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[ctx.accounts.adapter_position.authority_bump],
    ]];

    // TODO: replace with real CPI call
    // cpi::cpi_deposit(&ctx.accounts.protocol_program, ..., amount, signer_seeds)?;
    let _ = signer_seeds;

    // 4. Update share balance.
    ctx.accounts.adapter_position.shares = ctx.accounts.adapter_position.shares
        .checked_add(shares)
        .ok_or(error!(AdapterError::Overflow))?;

    // 5. REQUIRED: write shares_received to return data.
    set_return_data(&shares.to_le_bytes());
    Ok(())
}
