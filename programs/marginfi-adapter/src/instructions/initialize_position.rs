use anchor_lang::prelude::*;

use crate::{
    cpi::{self, MARGINFI_PROGRAM_ID},
    state::MarginfiAdapterPosition,
};

#[derive(Accounts)]
pub struct InitializePosition<'info> {
    /// The user initializing this position; pays for the PDA and the MarginFi account.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// Adapter-side position PDA. Created here.
    #[account(
        init,
        payer = owner,
        space = MarginfiAdapterPosition::SPACE,
        seeds = [MarginfiAdapterPosition::SEED, owner.key().as_ref()],
        bump,
    )]
    pub adapter_position: Account<'info, MarginfiAdapterPosition>,

    /// Virtual PDA — no data, used as the MarginFi authority signer in sub-CPIs.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [MarginfiAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump,
    )]
    pub marginfi_authority: UncheckedAccount<'info>,

    /// MarginFi group (read-only, passed to MarginFi CPI).
    /// CHECK: read-only, validated by MarginFi program
    pub marginfi_group: UncheckedAccount<'info>,

    /// The MarginFi account being created inside the MarginFi program.
    /// MarginFi's `marginfi_account_initialize` requires this to be a fresh
    /// keypair that signs its own initialization (it is `init`-ed with no seeds,
    /// not a PDA), so the client must generate it and pass it as a signer.
    #[account(mut)]
    pub marginfi_account: Signer<'info>,

    /// CHECK: must equal MARGINFI_PROGRAM_ID
    #[account(address = MARGINFI_PROGRAM_ID)]
    pub marginfi_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    let authority_bump = ctx.bumps.marginfi_authority;
    let owner_key = ctx.accounts.owner.key();

    let signer_seeds: &[&[&[u8]]] = &[&[
        MarginfiAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[authority_bump],
    ]];

    cpi::cpi_initialize_account(
        &ctx.accounts.marginfi_program,
        &ctx.accounts.marginfi_group,
        &ctx.accounts.marginfi_account,
        &ctx.accounts.marginfi_authority,
        &ctx.accounts.owner,
        &ctx.accounts.system_program,
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = owner_key;
    pos.marginfi_account = ctx.accounts.marginfi_account.key();
    pos.shares = 0;
    pos.authority_bump = authority_bump;
    pos.bump = ctx.bumps.adapter_position;

    Ok(())
}
