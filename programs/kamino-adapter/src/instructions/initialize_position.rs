use anchor_lang::prelude::*;

use crate::{
    cpi::{self, KLEND_PROGRAM_ID, MAIN_LENDING_MARKET, RENT_SYSVAR_ID, USER_METADATA_SEED},
    state::KaminoAdapterPosition,
};

/// Obligation vanilla seeds: [tag=0, id=0, kamino_authority, lending_market, system, system]
const TAG: u8 = 0;
const OBL_ID: u8 = 0;

#[derive(Accounts)]
pub struct InitializePosition<'info> {
    /// The user initializing this position; pays for PDAs.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// Adapter-side position PDA. Created here.
    #[account(
        init,
        payer = owner,
        space = KaminoAdapterPosition::SPACE,
        seeds = [KaminoAdapterPosition::SEED, owner.key().as_ref()],
        bump,
    )]
    pub adapter_position: Account<'info, KaminoAdapterPosition>,

    /// Virtual authority PDA — no data; signs sub-CPIs as Kamino obligation owner.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [KaminoAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump,
    )]
    pub kamino_authority: UncheckedAccount<'info>,

    /// Kamino Lending market (validated to be MAIN_LENDING_MARKET).
    /// CHECK: checked via address constraint
    #[account(address = MAIN_LENDING_MARKET)]
    pub lending_market: UncheckedAccount<'info>,

    /// Kamino user metadata PDA for kamino_authority.
    /// Seeds under KLEND_PROGRAM_ID: [b"user_meta", kamino_authority].
    /// May or may not exist already; we CPI init_user_metadata only if empty.
    /// CHECK: PDA derivation validated by seeds::program constraint
    #[account(
        mut,
        seeds = [USER_METADATA_SEED, kamino_authority.key().as_ref()],
        bump,
        seeds::program = KLEND_PROGRAM_ID,
    )]
    pub owner_user_metadata: UncheckedAccount<'info>,

    /// Kamino Obligation PDA for kamino_authority + main market.
    /// Seeds under KLEND_PROGRAM_ID: [tag, id, kamino_authority, lending_market, system, system].
    /// CHECK: PDA derivation validated by seeds::program constraint
    #[account(
        mut,
        seeds = [
            &[TAG],
            &[OBL_ID],
            kamino_authority.key().as_ref(),
            lending_market.key().as_ref(),
            system_program.key().as_ref(),
            system_program.key().as_ref(),
        ],
        bump,
        seeds::program = KLEND_PROGRAM_ID,
    )]
    pub obligation: UncheckedAccount<'info>,

    /// USDC Reserve in the Kamino lending market.
    /// Stored in adapter_position for validation in subsequent calls.
    /// CHECK: existence/ownership validated by Kamino on deposit/withdraw
    pub reserve: UncheckedAccount<'info>,

    /// CHECK: must equal KLEND_PROGRAM_ID
    #[account(address = KLEND_PROGRAM_ID)]
    pub klend_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,

    /// CHECK: Rent sysvar, passed to Kamino CPI
    #[account(address = RENT_SYSVAR_ID)]
    pub rent: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    let authority_bump = ctx.bumps.kamino_authority;
    let owner_key = ctx.accounts.owner.key();

    let signer_seeds: &[&[&[u8]]] = &[&[
        KaminoAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[authority_bump],
    ]];

    // Initialize user_metadata under Kamino only if not already created.
    if ctx.accounts.owner_user_metadata.data_is_empty() {
        cpi::cpi_init_user_metadata(
            &ctx.accounts.klend_program,
            &ctx.accounts.kamino_authority,
            &ctx.accounts.owner,
            &ctx.accounts.owner_user_metadata,
            &ctx.accounts.rent,
            &ctx.accounts.system_program,
            signer_seeds,
        )?;
    }

    // Create the Kamino Obligation for kamino_authority.
    cpi::cpi_init_obligation(
        &ctx.accounts.klend_program,
        &ctx.accounts.kamino_authority,
        &ctx.accounts.owner,
        &ctx.accounts.obligation,
        &ctx.accounts.lending_market,
        &ctx.accounts.system_program, // seed1 = SystemProgram for VanillaObligation
        &ctx.accounts.system_program, // seed2 = SystemProgram for VanillaObligation
        &ctx.accounts.owner_user_metadata,
        &ctx.accounts.rent,
        &ctx.accounts.system_program,
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = owner_key;
    pos.obligation = ctx.accounts.obligation.key();
    pos.reserve = ctx.accounts.reserve.key();
    pos.shares = 0;
    pos.authority_bump = authority_bump;
    pos.bump = ctx.bumps.adapter_position;

    Ok(())
}
