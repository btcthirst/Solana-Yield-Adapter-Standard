use anchor_lang::{prelude::*, solana_program::program::set_return_data};
use anchor_spl::token::{Token, TokenAccount};

use crate::{
    cpi::{self, JLP_MINT, JLP_POOL, PERP_PROGRAM_ID, USDC_CUSTODY, USDC_MINT},
    error::AdapterError,
    state::JupiterLpAdapterPosition,
};

/// Withdraw accounts.
///
/// Called indirectly via Dispatcher CPI (invoke), so `owner` is not declared Signer.
///
/// Remaining accounts:
///   [0] custody_oracle — price oracle for the USDC custody
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [JupiterLpAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, JupiterLpAdapterPosition>,

    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        mut,
        seeds = [JupiterLpAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub jlp_authority: UncheckedAccount<'info>,

    /// CHECK: validated by address constraint
    #[account(address = PERP_PROGRAM_ID)]
    pub perp_program: UncheckedAccount<'info>,

    /// CHECK: validated by address constraint
    #[account(mut, address = JLP_POOL)]
    pub pool: UncheckedAccount<'info>,

    /// CHECK: validated by address constraint
    #[account(mut, address = USDC_CUSTODY)]
    pub custody: UncheckedAccount<'info>,

    /// CHECK: USDC custody vault
    #[account(
        mut,
        seeds = [b"custody_token_account", JLP_POOL.as_ref(), USDC_MINT.as_ref()],
        bump,
        seeds::program = PERP_PROGRAM_ID,
    )]
    pub custody_token_account: UncheckedAccount<'info>,

    /// CHECK: Jupiter Perpetuals transferAuthority PDA
    #[account(
        seeds = [b"transfer_authority"],
        bump,
        seeds::program = PERP_PROGRAM_ID,
    )]
    pub transfer_authority: UncheckedAccount<'info>,

    /// CHECK: Jupiter Perpetuals state PDA
    #[account(
        mut,
        seeds = [b"perpetuals"],
        bump,
        seeds::program = PERP_PROGRAM_ID,
    )]
    pub perpetuals: UncheckedAccount<'info>,

    /// CHECK: JLP token mint
    #[account(mut, address = JLP_MINT)]
    pub jlp_mint: UncheckedAccount<'info>,

    /// Authority's JLP ATA — JLP is burned from here.
    /// Constrained to be the canonical ATA of jlp_authority for JLP_MINT.
    #[account(
        mut,
        associated_token::mint = jlp_mint,
        associated_token::authority = jlp_authority,
    )]
    pub authority_jlp_ata: Account<'info, TokenAccount>,

    /// CHECK: User's USDC ATA — receives redeemed USDC; ownership enforced by Jupiter CPI
    #[account(mut)]
    pub user_usdc_ata: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, anchor_spl::associated_token::AssociatedToken>,
}

/// `shares == 0` → withdraw all JLP; else withdraw exactly `shares` JLP tokens.
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.adapter_position.shares;
    require!(pos_shares > 0, AdapterError::InsufficientShares);

    let jlp_to_burn = if shares == 0 {
        pos_shares
    } else {
        require!(shares <= pos_shares, AdapterError::InsufficientShares);
        shares
    };

    let owner_key = ctx.accounts.owner.key();
    let auth_bump = ctx.accounts.adapter_position.authority_bump;
    let signer_seeds: &[&[&[u8]]] = &[&[
        JupiterLpAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[auth_bump],
    ]];

    let oracle = ctx
        .remaining_accounts
        .first()
        .cloned()
        .unwrap_or_else(|| ctx.accounts.perp_program.to_account_info());

    // CPI: removeLiquidity — burns JLP from authority ATA, sends USDC to user ATA.
    cpi::cpi_remove_liquidity(
        &ctx.accounts.perp_program,
        &ctx.accounts.jlp_authority,
        &ctx.accounts.user_usdc_ata,
        &ctx.accounts.authority_jlp_ata.to_account_info(),
        &ctx.accounts.transfer_authority,
        &ctx.accounts.perpetuals,
        &ctx.accounts.pool,
        &ctx.accounts.custody,
        &oracle,
        &ctx.accounts.custody_token_account,
        &ctx.accounts.jlp_mint,
        &ctx.accounts.token_program,
        jlp_to_burn,
        0, // no slippage protection in the reference adapter
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos
        .shares
        .checked_sub(jlp_to_burn)
        .ok_or(error!(AdapterError::InsufficientShares))?;

    set_return_data(&jlp_to_burn.to_le_bytes());
    Ok(())
}
