use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, USDC_BANK},
    error::AdapterError,
    state::MarginfiAdapterPosition,
};

/// Accounts for `deposit`.
///
/// Called indirectly via the Dispatcher's CPI (invoke, not invoke_signed),
/// so `owner` is not a Signer — only the adapter's own PDAs sign sub-CPIs.
#[derive(Accounts)]
pub struct Deposit<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [MarginfiAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MarginfiAdapterPosition>,

    /// Virtual signer PDA — no lamports needed; signs sub-CPI to MarginFi.
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

    /// CHECK: User's USDC ATA (source of funds)
    #[account(mut)]
    pub signer_token_account: UncheckedAccount<'info>,

    /// CHECK: MarginFi Bank USDC vault (destination)
    #[account(mut)]
    pub bank_liquidity_vault: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: must equal MARGINFI_PROGRAM_ID
    #[account(address = cpi::MARGINFI_PROGRAM_ID)]
    pub marginfi_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::InsufficientFunds);

    // Read asset_share_value before deposit to compute shares received.
    // shares = amount * ONE_I80F48 / asset_share_value
    let asset_share_value = cpi::read_asset_share_value(&ctx.accounts.bank)?;
    let shares_received = cpi::amount_to_shares(amount, asset_share_value)
        .ok_or(error!(AdapterError::Overflow))?;

    let owner_key = ctx.accounts.owner.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        MarginfiAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[ctx.accounts.adapter_position.authority_bump],
    ]];

    cpi::cpi_deposit(
        &ctx.accounts.marginfi_program,
        &ctx.accounts.marginfi_group,
        &ctx.accounts.marginfi_account,
        &ctx.accounts.marginfi_authority,
        &ctx.accounts.bank,
        &ctx.accounts.signer_token_account,
        &ctx.accounts.bank_liquidity_vault,
        &ctx.accounts.token_program,
        amount,
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos.shares
        .checked_add(shares_received)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&shares_received.to_le_bytes());
    Ok(())
}
