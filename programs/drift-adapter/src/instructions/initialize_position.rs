use anchor_lang::prelude::*;

use crate::{
    cpi::{self, DRIFT_PROGRAM_ID, DRIFT_STATE, MARKET_INDEX, USDC_SPOT_MARKET},
    state::DriftAdapterPosition,
};

/// Initialize the adapter position and the user's Drift IF accounts.
///
/// Calls `initialize_user_stats` (only if user_stats doesn't exist yet, so
/// existing Drift users are supported) then `initialize_insurance_fund_stake`.
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

    /// Drift UserStats PDA. Seeds: [b"user_stats", owner] @ DRIFT_PROGRAM_ID.
    /// May already exist if the user already uses Drift directly.
    /// CHECK: PDA derivation validated by seeds constraint; Drift validates the rest
    #[account(
        mut,
        seeds = [b"user_stats", owner.key().as_ref()],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub user_stats: UncheckedAccount<'info>,

    /// InsuranceFundStake PDA. Seeds: [b"insurance_fund_stake", owner, market_index_le] @ DRIFT_PROGRAM_ID.
    /// CHECK: PDA derivation validated by seeds constraint; Drift validates the rest
    #[account(
        mut,
        seeds = [b"insurance_fund_stake", owner.key().as_ref(), &[0u8, 0u8]],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub insurance_fund_stake: UncheckedAccount<'info>,

    /// CHECK: Drift state account; validated by address constraint. mut because
    /// initialize_user_stats increments the global user count stored in state.
    #[account(mut, address = DRIFT_STATE)]
    pub state: UncheckedAccount<'info>,

    /// CHECK: USDC spot market; validated by address constraint
    #[account(address = USDC_SPOT_MARKET)]
    pub spot_market: UncheckedAccount<'info>,

    /// CHECK: must equal DRIFT_PROGRAM_ID
    #[account(address = DRIFT_PROGRAM_ID)]
    pub drift_program: UncheckedAccount<'info>,

    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    // Only initialize UserStats if it doesn't exist yet (supports existing Drift users).
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

    cpi::cpi_initialize_insurance_fund_stake(
        &ctx.accounts.drift_program,
        &ctx.accounts.spot_market,
        &ctx.accounts.insurance_fund_stake,
        &ctx.accounts.user_stats,
        &ctx.accounts.owner,
        &ctx.accounts.owner,
        &ctx.accounts.state,
        &ctx.accounts.rent.to_account_info(),
        &ctx.accounts.system_program,
        MARKET_INDEX,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = ctx.accounts.owner.key();
    pos.if_shares = 0;
    pos.market_index = MARKET_INDEX;
    pos.pending_withdrawal = false;
    pos.withdrawal_request_shares = 0;
    pos.withdrawal_request_ts = 0;
    pos.bump = ctx.bumps.adapter_position;

    Ok(())
}
