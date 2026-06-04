use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, DRIFT_PROGRAM_ID, DRIFT_SIGNER, DRIFT_STATE, MARKET_INDEX, USDC_IF_VAULT, USDC_SPOT_MARKET},
    error::AdapterError,
    state::DriftAdapterPosition,
};

/// Two-step Drift IF unstake.
///
/// First call  → `request_remove_insurance_fund_stake`: starts the cooldown,
///               sets `pending_withdrawal = true`, returns 0 shares.
/// Second call → `remove_insurance_fund_stake` (after cooldown elapses):
///               transfers USDC to the user, returns shares_removed as u64 LE.
///
/// Called indirectly via Dispatcher CPI (`invoke`): `owner` is UncheckedAccount,
/// signature propagated from the Dispatcher tx through the is_signer flag.
///
/// All accounts must be present on both calls; only a subset is used per step.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// CHECK: position owner; is_signer propagated from Dispatcher via invoke
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [DriftAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, DriftAdapterPosition>,

    /// CHECK: Drift state; validated by address constraint
    #[account(address = DRIFT_STATE)]
    pub state: UncheckedAccount<'info>,

    /// CHECK: USDC spot market; validated by address constraint (used in both steps)
    #[account(mut, address = USDC_SPOT_MARKET)]
    pub spot_market: UncheckedAccount<'info>,

    /// CHECK: user's InsuranceFundStake PDA
    #[account(
        mut,
        seeds = [b"insurance_fund_stake", owner.key().as_ref(), &[0u8, 0u8]],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub insurance_fund_stake: UncheckedAccount<'info>,

    /// CHECK: user's Drift UserStats PDA
    #[account(
        mut,
        seeds = [b"user_stats", owner.key().as_ref()],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub user_stats: UncheckedAccount<'info>,

    // ── Step-2 only accounts (must still be passed on step 1) ────────────────

    /// CHECK: USDC IF vault (source of USDC on final withdraw); validated by address constraint
    #[account(mut, address = USDC_IF_VAULT)]
    pub insurance_fund_vault: UncheckedAccount<'info>,

    /// CHECK: Drift vault authority PDA; validated by address constraint
    #[account(address = DRIFT_SIGNER)]
    pub drift_signer: UncheckedAccount<'info>,

    /// CHECK: user's USDC ATA (receives withdrawn USDC on step 2)
    #[account(mut)]
    pub user_token_account: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: must equal DRIFT_PROGRAM_ID
    #[account(address = DRIFT_PROGRAM_ID)]
    pub drift_program: UncheckedAccount<'info>,
}

pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, _shares: u64) -> Result<()> {
    let pos = &mut ctx.accounts.adapter_position;

    if pos.pending_withdrawal {
        // ── Step 2: finalize withdrawal after cooldown ───────────────────────
        let now = Clock::get()?.unix_timestamp;
        let cooldown = cpi::read_unstaking_period(&ctx.accounts.spot_market)?;

        require!(
            now >= pos.withdrawal_request_ts + cooldown,
            AdapterError::CooldownActive
        );

        cpi::cpi_remove_insurance_fund_stake(
            &ctx.accounts.drift_program,
            &ctx.accounts.state,
            &ctx.accounts.spot_market,
            &ctx.accounts.insurance_fund_vault,
            &ctx.accounts.insurance_fund_stake,
            &ctx.accounts.user_stats,
            &ctx.accounts.owner,
            &ctx.accounts.drift_signer,
            &ctx.accounts.user_token_account,
            &ctx.accounts.token_program,
            MARKET_INDEX,
        )?;

        let shares_removed = pos.withdrawal_request_shares;
        pos.if_shares = pos
            .if_shares
            .checked_sub(shares_removed)
            .unwrap_or(0);
        pos.pending_withdrawal = false;
        pos.withdrawal_request_shares = 0;
        pos.withdrawal_request_ts = 0;

        let shares_u64 =
            u64::try_from(shares_removed).map_err(|_| error!(AdapterError::Overflow))?;

        set_return_data(&shares_u64.to_le_bytes());
    } else {
        // ── Step 1: initiate cooldown ────────────────────────────────────────
        require!(pos.if_shares > 0, AdapterError::InsufficientShares);

        cpi::cpi_request_remove_insurance_fund_stake(
            &ctx.accounts.drift_program,
            &ctx.accounts.state,
            &ctx.accounts.spot_market,
            &ctx.accounts.insurance_fund_stake,
            &ctx.accounts.user_stats,
            &ctx.accounts.owner,
            MARKET_INDEX,
        )?;

        let cooldown = cpi::read_unstaking_period(&ctx.accounts.spot_market)?;
        let now = Clock::get()?.unix_timestamp;

        pos.pending_withdrawal = true;
        pos.withdrawal_request_shares = pos.if_shares;
        pos.withdrawal_request_ts = now;

        msg!(
            "IF unstake requested: ready_at={}",
            now + cooldown
        );

        // Return 0 — shares not yet removed; Dispatcher should not decrement position.
        set_return_data(&0u64.to_le_bytes());
    }

    Ok(())
}
