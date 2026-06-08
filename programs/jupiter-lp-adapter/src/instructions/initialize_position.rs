use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{
    cpi::{JLP_MINT, JLP_POOL, PERP_PROGRAM_ID, USDC_CUSTODY, USDC_MINT},
    state::JupiterLpAdapterPosition,
};

#[derive(Accounts)]
pub struct InitializePosition<'info> {
    /// The user initializing this position; pays for PDAs and ATAs.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// Adapter-side position PDA. Created here.
    #[account(
        init,
        payer = owner,
        space = JupiterLpAdapterPosition::SPACE,
        seeds = [JupiterLpAdapterPosition::SEED, owner.key().as_ref()],
        bump,
    )]
    pub adapter_position: Account<'info, JupiterLpAdapterPosition>,

    /// Virtual authority PDA — no data; signs addLiquidity/removeLiquidity CPIs.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [JupiterLpAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump,
    )]
    pub jlp_authority: UncheckedAccount<'info>,

    /// JLP token ATA for the authority PDA. Created here.
    /// Holds JLP tokens on behalf of the user throughout the position lifetime.
    #[account(
        init,
        payer = owner,
        associated_token::mint = jlp_mint,
        associated_token::authority = jlp_authority,
    )]
    pub authority_jlp_ata: Account<'info, TokenAccount>,

    /// USDC ATA for the authority PDA. Created here.
    /// Used as transient USDC staging account during deposit.
    #[account(
        init,
        payer = owner,
        associated_token::mint = usdc_mint,
        associated_token::authority = jlp_authority,
    )]
    pub authority_usdc_ata: Account<'info, TokenAccount>,

    /// JLP token mint. Validated against JLP_MINT constant.
    /// CHECK: address constraint enforces correct mint
    #[account(address = JLP_MINT)]
    pub jlp_mint: Account<'info, Mint>,

    /// USDC mint. Validated against USDC_MINT constant.
    /// CHECK: address constraint enforces correct mint
    #[account(address = USDC_MINT)]
    pub usdc_mint: Account<'info, Mint>,

    /// CHECK: validated by address constraint
    #[account(address = PERP_PROGRAM_ID)]
    pub perp_program: UncheckedAccount<'info>,

    /// Validates USDC custody is the expected account.
    /// CHECK: address constraint
    #[account(address = USDC_CUSTODY)]
    pub usdc_custody: UncheckedAccount<'info>,

    /// CHECK: validated by address constraint
    #[account(address = JLP_POOL)]
    pub pool: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = ctx.accounts.owner.key();
    pos.shares = 0;
    pos.authority_bump = ctx.bumps.jlp_authority;
    pos.bump = ctx.bumps.adapter_position;
    Ok(())
}
