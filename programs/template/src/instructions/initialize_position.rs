use anchor_lang::prelude::*;

use crate::state::TemplateAdapterPosition;

#[derive(Accounts)]
pub struct InitializePosition<'info> {
    /// The user initializing this position; pays for the PDA.
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        init,
        payer = owner,
        space = TemplateAdapterPosition::SPACE,
        seeds = [TemplateAdapterPosition::SEED, owner.key().as_ref()],
        bump,
    )]
    pub adapter_position: Account<'info, TemplateAdapterPosition>,

    /// Virtual signer PDA — no data, used to sign sub-CPIs.
    /// CHECK: validated by seeds constraint
    #[account(
        seeds = [TemplateAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    // TODO: add protocol-specific accounts for one-time setup
    // (e.g. creating a user account inside the target protocol)

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    let authority_bump = ctx.bumps.authority;
    let owner = ctx.accounts.owner.key();

    let signer_seeds: &[&[&[u8]]] = &[&[
        TemplateAdapterPosition::AUTH_SEED,
        owner.as_ref(),
        &[authority_bump],
    ]];

    // TODO: CPI to open a user account in the target protocol.
    // cpi::cpi_open_account(&ctx.accounts.protocol_program, ..., signer_seeds)?;
    let _ = signer_seeds; // remove once CPI is implemented

    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = owner;
    pos.shares = 0;
    pos.authority_bump = authority_bump;
    pos.bump = ctx.bumps.adapter_position;
    Ok(())
}
