use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, DRIFT_PROGRAM_ID, DRIFT_SIGNER, DRIFT_STATE, MARKET_INDEX, USDC_IF_VAULT, USDC_SPOT_MARKET},
    error::AdapterError,
    state::DriftAdapterPosition,
};

/// Deposit (stake) USDC into the Drift Insurance Fund.
///
/// Called indirectly via Dispatcher CPI (`invoke`, not `invoke_signed`),
/// so `owner` is UncheckedAccount — their signature is propagated from the
/// Dispatcher transaction through the `is_signer` flag on the AccountInfo.
///
/// Returns `if_shares_received` as u64 LE via set_return_data.
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

    /// CHECK: USDC spot market; validated by address constraint
    #[account(mut, address = USDC_SPOT_MARKET)]
    pub spot_market: UncheckedAccount<'info>,

    /// CHECK: USDC insurance fund vault (destination of staked USDC); validated by address constraint
    #[account(mut, address = USDC_IF_VAULT)]
    pub insurance_fund_vault: UncheckedAccount<'info>,

    /// CHECK: user's InsuranceFundStake PDA; PDA derivation validated by seeds constraint
    #[account(
        mut,
        seeds = [b"insurance_fund_stake", owner.key().as_ref(), &[0u8, 0u8]],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub insurance_fund_stake: UncheckedAccount<'info>,

    /// CHECK: user's Drift UserStats PDA; PDA derivation validated by seeds constraint
    #[account(
        mut,
        seeds = [b"user_stats", owner.key().as_ref()],
        bump,
        seeds::program = DRIFT_PROGRAM_ID,
    )]
    pub user_stats: UncheckedAccount<'info>,

    /// USDC spot market collateral vault (for revenue settle during stake).
    /// Seeds: [b"spot_market_vault", market_index_le] @ DRIFT_PROGRAM_ID.
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

    /// CHECK: user's USDC ATA (source of funds for staking)
    #[account(mut)]
    pub user_token_account: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: must equal DRIFT_PROGRAM_ID
    #[account(address = DRIFT_PROGRAM_ID)]
    pub drift_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::InsufficientShares);

    let old_shares = ctx.accounts.adapter_position.if_shares;

    cpi::cpi_add_insurance_fund_stake(
        &ctx.accounts.drift_program,
        &ctx.accounts.state,
        &ctx.accounts.spot_market,
        &ctx.accounts.insurance_fund_stake,
        &ctx.accounts.user_stats,
        &ctx.accounts.owner,
        &ctx.accounts.spot_market_vault,
        &ctx.accounts.insurance_fund_vault,
        &ctx.accounts.drift_signer,
        &ctx.accounts.user_token_account,
        &ctx.accounts.token_program,
        MARKET_INDEX,
        amount,
    )?;

    let new_shares = cpi::read_if_shares(&ctx.accounts.insurance_fund_stake)?;
    let shares_received = new_shares.saturating_sub(old_shares);

    let pos = &mut ctx.accounts.adapter_position;
    pos.if_shares = new_shares;

    let shares_u64 =
        u64::try_from(shares_received).map_err(|_| error!(AdapterError::Overflow))?;

    set_return_data(&shares_u64.to_le_bytes());
    Ok(())
}
