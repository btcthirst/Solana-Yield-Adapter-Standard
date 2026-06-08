use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{cpi, error::AdapterError, state::TemplateAdapterPosition};

/// Accounts for `withdraw`.
///
/// Called via Dispatcher CPI (invoke) — `owner` is NOT a Signer.
/// Explicit `'info` lifetime is required when remaining_accounts are
/// forwarded into a sub-CPI via `.to_vec()`.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// CHECK: used for PDA seed derivation only
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [TemplateAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, TemplateAdapterPosition>,

    /// CHECK: authority signer PDA
    #[account(
        seeds = [TemplateAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub authority: UncheckedAccount<'info>,

    // TODO: add protocol-specific accounts (destination token account, vaults, ...)
}

/// `shares == 0` withdraws the entire position.
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.adapter_position.shares;

    // Determine how many shares and how many asset lamports to withdraw.
    let rate = 1_000_000u64; // TODO: cpi::read_exchange_rate(&ctx.accounts.protocol_state)?
    let (shares_to_remove, amount_to_withdraw) = if shares == 0 {
        let amount = cpi::shares_to_amount(pos_shares, rate)
            .ok_or(error!(AdapterError::Overflow))?;
        (pos_shares, amount)
    } else {
        require!(shares <= pos_shares, AdapterError::InsufficientShares);
        let amount = cpi::shares_to_amount(shares, rate)
            .ok_or(error!(AdapterError::Overflow))?;
        (shares, amount)
    };

    let owner_key = ctx.accounts.owner.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        TemplateAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[ctx.accounts.adapter_position.authority_bump],
    ]];

    // TODO: replace with real CPI call
    // cpi::cpi_withdraw(&ctx.accounts.protocol_program, ..., amount_to_withdraw, signer_seeds)?;
    let _ = (amount_to_withdraw, signer_seeds);

    ctx.accounts.adapter_position.shares = ctx.accounts.adapter_position.shares
        .checked_sub(shares_to_remove)
        .ok_or(error!(AdapterError::InsufficientShares))?;

    // REQUIRED: write shares_removed to return data.
    // For cooldown protocols: write 0u64 and track the pending withdrawal in state.
    set_return_data(&shares_to_remove.to_le_bytes());
    Ok(())
}
