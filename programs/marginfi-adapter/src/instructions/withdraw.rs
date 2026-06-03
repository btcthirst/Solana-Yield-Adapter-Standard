use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, USDC_BANK, VAULT_AUTH_SEED, MARGINFI_PROGRAM_ID},
    error::AdapterError,
    state::MarginfiAdapterPosition,
};

/// Accounts for `withdraw`.
///
/// Oracle accounts required by MarginFi for health-check are passed via
/// `remaining_accounts` and forwarded to the MarginFi CPI.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [MarginfiAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MarginfiAdapterPosition>,

    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [MarginfiAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub marginfi_authority: UncheckedAccount<'info>,

    /// CHECK: MarginFi group (read-only, forwarded to MarginFi)
    pub marginfi_group: UncheckedAccount<'info>,

    /// CHECK: MarginFi account owned by this adapter's authority
    #[account(
        mut,
        constraint = marginfi_account.key() == adapter_position.marginfi_account @ AdapterError::ProtocolError,
    )]
    pub marginfi_account: UncheckedAccount<'info>,

    /// CHECK: USDC Bank; read for asset_share_value and forwarded to MarginFi
    #[account(
        mut,
        constraint = bank.key() == USDC_BANK @ AdapterError::ProtocolError,
    )]
    pub bank: UncheckedAccount<'info>,

    /// CHECK: User's USDC ATA (receives the withdrawn tokens)
    #[account(mut)]
    pub destination_token_account: UncheckedAccount<'info>,

    /// MarginFi vault authority PDA — seeds: [b"liquidity_vault_auth", bank].
    /// CHECK: PDA derivation validated by seeds::program constraint
    #[account(
        seeds = [VAULT_AUTH_SEED, bank.key().as_ref()],
        bump,
        seeds::program = MARGINFI_PROGRAM_ID,
    )]
    pub bank_liquidity_vault_authority: UncheckedAccount<'info>,

    /// CHECK: MarginFi Bank USDC vault (source of funds)
    #[account(mut)]
    pub bank_liquidity_vault: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: must equal MARGINFI_PROGRAM_ID
    #[account(address = MARGINFI_PROGRAM_ID)]
    pub marginfi_program: UncheckedAccount<'info>,
}

/// `shares == 0` → withdraw all; else withdraw the requested share amount.
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.adapter_position.shares;

    let (withdraw_all, amount_to_withdraw, shares_removed) = if shares == 0 {
        // Withdraw all: compute USDC amount from stored shares
        let asset_share_value = cpi::read_asset_share_value(&ctx.accounts.bank)?;
        let amount = cpi::shares_to_amount(pos_shares, asset_share_value)
            .ok_or(error!(AdapterError::Overflow))?;
        (true, amount, pos_shares)
    } else {
        require!(shares <= pos_shares, AdapterError::InsufficientShares);
        let asset_share_value = cpi::read_asset_share_value(&ctx.accounts.bank)?;
        let amount = cpi::shares_to_amount(shares, asset_share_value)
            .ok_or(error!(AdapterError::Overflow))?;
        (false, amount, shares)
    };

    let owner_key = ctx.accounts.owner.key();
    let authority_bump = ctx.accounts.adapter_position.authority_bump;
    let signer_seeds: &[&[&[u8]]] = &[&[
        MarginfiAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[authority_bump],
    ]];

    // Collect oracle remaining_accounts for MarginFi health check.
    let oracle_accounts: Vec<AccountInfo<'info>> = ctx.remaining_accounts.to_vec();

    cpi::cpi_withdraw(
        &ctx.accounts.marginfi_program,
        &ctx.accounts.marginfi_group,
        &ctx.accounts.marginfi_account,
        &ctx.accounts.marginfi_authority,
        &ctx.accounts.bank,
        &ctx.accounts.destination_token_account,
        &ctx.accounts.bank_liquidity_vault_authority,
        &ctx.accounts.bank_liquidity_vault,
        &ctx.accounts.token_program,
        &oracle_accounts,
        amount_to_withdraw,
        withdraw_all,
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos.shares
        .checked_sub(shares_removed)
        .ok_or(error!(AdapterError::InsufficientShares))?;

    set_return_data(&shares_removed.to_le_bytes());
    Ok(())
}
