use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{cpi::SYRUP_USDC_MINT, state::MapleAdapterPosition};

#[derive(Accounts)]
pub struct InitializePosition<'info> {
    /// Wallet initializing the position; pays for all PDAs/ATAs.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// Adapter position PDA. Created here.
    #[account(
        init,
        payer = owner,
        space = MapleAdapterPosition::SPACE,
        seeds = [MapleAdapterPosition::SEED, owner.key().as_ref()],
        bump,
    )]
    pub adapter_position: Account<'info, MapleAdapterPosition>,

    /// Virtual authority PDA — no data; signs token transfers on behalf of the position.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [MapleAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump,
    )]
    pub maple_authority: UncheckedAccount<'info>,

    /// syrupUSDC ATA for `maple_authority`. Created here.
    /// Holds syrupUSDC on behalf of the user for the lifetime of the position.
    #[account(
        init,
        payer = owner,
        associated_token::mint = syrup_usdc_mint,
        associated_token::authority = maple_authority,
    )]
    pub authority_syrup_ata: Account<'info, TokenAccount>,

    /// syrupUSDC mint. Validated against SYRUP_USDC_MINT constant.
    #[account(address = SYRUP_USDC_MINT)]
    pub syrup_usdc_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = ctx.accounts.owner.key();
    pos.shares = 0;
    pos.authority_bump = ctx.bumps.maple_authority;
    pos.bump = ctx.bumps.adapter_position;
    Ok(())
}
