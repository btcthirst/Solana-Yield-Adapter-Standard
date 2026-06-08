use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, DRIFT_PROGRAM_ID, DRIFT_STATE, MARKET_INDEX},
    error::AdapterError,
    state::DriftAdapterPosition,
};

/// Deposit USDC into the Drift spot market (lending).
///
/// Called indirectly via Dispatcher CPI (`invoke`, not `invoke_signed`),
/// so `owner` is UncheckedAccount — their signature is propagated from the
/// Dispatcher transaction through the `is_signer` flag on the AccountInfo.
///
/// Returns `amount` as u64 LE via set_return_data.
#[derive(Accounts)]
pub struct Deposit<'info> {
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

    /// USDC spot market vault (receives deposited USDC).
    /// Seeds: ["spot_market_vault", 0u16_le] @ DRIFT_PROGRAM_ID.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        mut,
        seeds = [b"spot_market_vault", &[0u8, 0u8]],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub spot_market_vault: UncheckedAccount<'info>,

    /// CHECK: user's USDC ATA (source of funds)
    #[account(mut)]
    pub user_token_account: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: must equal DRIFT_PROGRAM_ID
    #[account(address = DRIFT_PROGRAM_ID)]
    pub drift_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::ZeroAmount);

    cpi::cpi_deposit(
        &ctx.accounts.drift_program,
        &ctx.accounts.state,
        &ctx.accounts.drift_user,
        &ctx.accounts.user_stats,
        &ctx.accounts.owner,
        &ctx.accounts.spot_market_vault,
        &ctx.accounts.user_token_account,
        &ctx.accounts.token_program,
        MARKET_INDEX,
        amount,
    )?;

    ctx.accounts.adapter_position.deposited_amount = ctx
        .accounts
        .adapter_position
        .deposited_amount
        .checked_add(amount)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&amount.to_le_bytes());
    Ok(())
}
