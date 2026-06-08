use anchor_lang::prelude::*;

use crate::{
    cpi::{self, DRIFT_PROGRAM_ID, DRIFT_STATE, MARKET_INDEX},
    state::DriftAdapterPosition,
};

/// Initialize the adapter position and the user's Drift spot-market accounts.
///
/// Calls `initialize_user_stats` (skipped if already exists, supports existing Drift users),
/// then `initialize_user` (sub_account_id=0).
/// Must be called directly by the user — not via Dispatcher.
#[derive(Accounts)]
pub struct InitializePosition<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        init,
        payer = owner,
        space = DriftAdapterPosition::SPACE,
        seeds = [DriftAdapterPosition::SEED, owner.key().as_ref()],
        bump,
    )]
    pub adapter_position: Account<'info, DriftAdapterPosition>,

    /// Drift User PDA (sub_account_id=0).
    /// Seeds: ["user", owner, 0u16_le] @ DRIFT_PROGRAM_ID.
    /// CHECK: PDA derivation validated by seeds constraint; Drift validates the rest.
    #[account(
        mut,
        seeds = [b"user", owner.key().as_ref(), &[0u8, 0u8]],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub drift_user: UncheckedAccount<'info>,

    /// Drift UserStats PDA. Seeds: ["user_stats", owner] @ DRIFT_PROGRAM_ID.
    /// May already exist if the user already uses Drift directly.
    /// CHECK: PDA derivation validated by seeds constraint; Drift validates the rest.
    #[account(
        mut,
        seeds = [b"user_stats", owner.key().as_ref()],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub user_stats: UncheckedAccount<'info>,

    /// CHECK: Drift state account; mut because initialize_user_stats/user increment counters.
    #[account(mut, address = DRIFT_STATE)]
    pub state: UncheckedAccount<'info>,

    /// CHECK: must equal DRIFT_PROGRAM_ID
    #[account(address = DRIFT_PROGRAM_ID)]
    pub drift_program: UncheckedAccount<'info>,

    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    if ctx.accounts.user_stats.data_is_empty() {
        cpi::cpi_initialize_user_stats(
            &ctx.accounts.drift_program,
            &ctx.accounts.user_stats,
            &ctx.accounts.state,
            &ctx.accounts.owner,
            &ctx.accounts.owner,
            &ctx.accounts.rent.to_account_info(),
            &ctx.accounts.system_program,
        )?;
    }

    if ctx.accounts.drift_user.data_is_empty() {
        cpi::cpi_initialize_user(
            &ctx.accounts.drift_program,
            &ctx.accounts.drift_user,
            &ctx.accounts.user_stats,
            &ctx.accounts.state,
            &ctx.accounts.owner,
            &ctx.accounts.owner,
            &ctx.accounts.rent.to_account_info(),
            &ctx.accounts.system_program,
            MARKET_INDEX, // sub_account_id = 0
        )?;
    }

    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = ctx.accounts.owner.key();
    pos.deposited_amount = 0;
    pos.bump = ctx.bumps.adapter_position;

    Ok(())
}
