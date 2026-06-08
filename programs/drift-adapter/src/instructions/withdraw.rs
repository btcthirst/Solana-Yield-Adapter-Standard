use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, DRIFT_PROGRAM_ID, DRIFT_SIGNER, DRIFT_STATE, MARKET_INDEX},
    error::AdapterError,
    state::DriftAdapterPosition,
};

/// Withdraw USDC from the Drift spot market.
///
/// Single-step withdrawal (no cooldown, unlike IF staking).
/// Called indirectly via Dispatcher CPI (`invoke`): `owner` is UncheckedAccount,
/// signature propagated from the Dispatcher tx through the is_signer flag.
///
/// `_shares` is accepted for interface compatibility with the Dispatcher standard
/// but ignored — always withdraws the full deposited amount.
///
/// Returns `deposited_amount` as u64 LE via set_return_data.
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

    /// Drift User PDA (sub_account_id=0). Seeds: ["user", owner, 0u16_le] @ DRIFT_PROGRAM_ID.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        mut,
        seeds = [b"user", owner.key().as_ref(), &[0u8, 0u8]],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub drift_user: UncheckedAccount<'info>,

    /// Drift UserStats PDA. Seeds: ["user_stats", owner] @ DRIFT_PROGRAM_ID.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        mut,
        seeds = [b"user_stats", owner.key().as_ref()],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub user_stats: UncheckedAccount<'info>,

    /// USDC spot market vault (source of withdrawn USDC).
    /// Seeds: ["spot_market_vault", 0u16_le] @ DRIFT_PROGRAM_ID.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        mut,
        seeds = [b"spot_market_vault", &[0u8, 0u8]],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub spot_market_vault: UncheckedAccount<'info>,

    /// CHECK: Drift vault authority PDA; validated by address constraint
    #[account(address = DRIFT_SIGNER)]
    pub drift_signer: UncheckedAccount<'info>,

    /// CHECK: user's USDC ATA (receives withdrawn USDC)
    #[account(mut)]
    pub user_token_account: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: must equal DRIFT_PROGRAM_ID
    #[account(address = DRIFT_PROGRAM_ID)]
    pub drift_program: UncheckedAccount<'info>,
}

pub fn handler<'info>(
    ctx: Context<'info, Withdraw<'info>>,
    _shares: u64,
) -> Result<()> {
    let amount = ctx.accounts.adapter_position.deposited_amount;
    require!(amount > 0, AdapterError::InsufficientShares);

    cpi::cpi_withdraw(
        &ctx.accounts.drift_program,
        &ctx.accounts.state,
        &ctx.accounts.drift_user,
        &ctx.accounts.user_stats,
        &ctx.accounts.owner,
        &ctx.accounts.spot_market_vault,
        &ctx.accounts.drift_signer,
        &ctx.accounts.user_token_account,
        &ctx.accounts.token_program,
        MARKET_INDEX,
        amount,
    )?;

    ctx.accounts.adapter_position.deposited_amount = 0;

    set_return_data(&amount.to_le_bytes());
    Ok(())
}
