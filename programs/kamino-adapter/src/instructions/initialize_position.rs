use anchor_lang::prelude::*;

use crate::{
    cpi::{
        self, FARMS_PROGRAM_ID, KLEND_PROGRAM_ID, MAIN_LENDING_MARKET, RENT_SYSVAR_ID,
        USER_METADATA_SEED,
    },
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
    #[account(mut)]
    pub reserve: UncheckedAccount<'info>,

    /// Lending market authority PDA (Kamino-owned), needed to init the farm state.
    /// CHECK: validated by Kamino; seeds = [b"lma", lending_market] @ KLEND_PROGRAM_ID
    pub lending_market_authority: UncheckedAccount<'info>,

    /// The reserve's collateral farm state (reserve.farmCollateral).
    /// CHECK: validated by the Kamino farms program against the reserve
    #[account(mut)]
    pub reserve_farm_state: UncheckedAccount<'info>,

    /// The obligation's farm-user-state, created here for the V2 deposit path.
    /// Seeds under FARMS_PROGRAM_ID: [b"user", reserve_farm_state, obligation].
    /// CHECK: PDA derivation validated by seeds::program constraint
    #[account(
        mut,
        seeds = [b"user", reserve_farm_state.key().as_ref(), obligation.key().as_ref()],
        bump,
        seeds::program = FARMS_PROGRAM_ID,
    )]
    pub obligation_farm: UncheckedAccount<'info>,

    /// CHECK: must equal FARMS_PROGRAM_ID
    #[account(address = FARMS_PROGRAM_ID)]
    pub farms_program: UncheckedAccount<'info>,

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

    // Create the obligation's farm-user-state (mode 0 = collateral farm) so the
    // V2 deposit/withdraw can update the farm. Permissionless — payer (owner) signs.
    cpi::cpi_init_obligation_farms_for_reserve(
        &ctx.accounts.klend_program,
        &ctx.accounts.owner,
        &ctx.accounts.kamino_authority,
        &ctx.accounts.obligation,
        &ctx.accounts.lending_market_authority,
        &ctx.accounts.reserve,
        &ctx.accounts.reserve_farm_state,
        &ctx.accounts.obligation_farm,
        &ctx.accounts.lending_market,
        &ctx.accounts.farms_program,
        &ctx.accounts.rent,
        &ctx.accounts.system_program,
        0, // mode 0 = collateral farm
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
