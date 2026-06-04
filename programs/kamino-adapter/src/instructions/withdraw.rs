use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, INSTRUCTIONS_SYSVAR_ID, KLEND_PROGRAM_ID, MAIN_LENDING_MARKET},
    error::AdapterError,
    state::KaminoAdapterPosition,
};

/// Withdraw accounts.
///
/// Called indirectly via Dispatcher CPI (invoke, not invoke_signed),
/// so `owner` is not a Signer.
///
/// Remaining accounts (optional, forwarded to refresh_reserve):
///   [0] pyth_oracle
///   [1] switchboard_price_oracle
///   [2] switchboard_twap_oracle
///   [3] scope_prices
/// Omit or pass KLEND_PROGRAM_ID for oracles you don't have.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [KaminoAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, KaminoAdapterPosition>,

    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [KaminoAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub kamino_authority: UncheckedAccount<'info>,

    /// CHECK: Kamino lending market; validated to be main market
    #[account(address = MAIN_LENDING_MARKET)]
    pub lending_market: UncheckedAccount<'info>,

    /// Lending market authority PDA (Kamino-owned).
    /// CHECK: validated by Kamino; seeds = [b"lma", lending_market] @ KLEND_PROGRAM_ID
    pub lending_market_authority: UncheckedAccount<'info>,

    /// CHECK: Kamino Obligation PDA; validated via adapter_position
    #[account(
        mut,
        constraint = obligation.key() == adapter_position.obligation @ AdapterError::ProtocolError,
    )]
    pub obligation: UncheckedAccount<'info>,

    /// CHECK: USDC Reserve; validated via adapter_position
    #[account(
        mut,
        constraint = reserve.key() == adapter_position.reserve @ AdapterError::InvalidReserve,
    )]
    pub reserve: UncheckedAccount<'info>,

    /// CHECK: Reserve liquidity mint (USDC)
    pub reserve_liquidity_mint: UncheckedAccount<'info>,

    /// CHECK: Reserve's collateral supply vault (source of ctokens — mut)
    #[account(mut)]
    pub reserve_source_collateral: UncheckedAccount<'info>,

    /// CHECK: kUSDC collateral mint (mut — burns ctokens)
    #[account(mut)]
    pub reserve_collateral_mint: UncheckedAccount<'info>,

    /// CHECK: Reserve's USDC vault (source of USDC — mut)
    #[account(mut)]
    pub reserve_liquidity_supply: UncheckedAccount<'info>,

    /// CHECK: User's USDC ATA (receives withdrawn USDC)
    #[account(mut)]
    pub user_destination_liquidity: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_SYSVAR_ID)]
    pub instruction_sysvar: UncheckedAccount<'info>,

    /// CHECK: must equal KLEND_PROGRAM_ID
    #[account(address = KLEND_PROGRAM_ID)]
    pub klend_program: UncheckedAccount<'info>,
}

/// `shares == 0` → withdraw all ctokens; else withdraw the requested ctoken count.
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.adapter_position.shares;
    require!(pos_shares > 0, AdapterError::InsufficientShares);

    let ctokens_to_withdraw = if shares == 0 {
        pos_shares
    } else {
        require!(shares <= pos_shares, AdapterError::InsufficientShares);
        shares
    };

    // Build oracle accounts from remaining_accounts[0..4].
    let rem = ctx.remaining_accounts;
    let oracle0 = rem.get(0).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());
    let oracle1 = rem.get(1).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());
    let oracle2 = rem.get(2).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());
    let oracle3 = rem.get(3).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());

    // CPI 1: refresh_reserve.
    cpi::cpi_refresh_reserve(
        &ctx.accounts.klend_program,
        &ctx.accounts.reserve,
        &ctx.accounts.lending_market,
        [oracle0, oracle1, oracle2, oracle3],
    )?;

    // CPI 2: refresh_obligation (required before withdraw for health check).
    // Pass the USDC reserve as the obligation's deposit reserve.
    cpi::cpi_refresh_obligation(
        &ctx.accounts.klend_program,
        &ctx.accounts.lending_market,
        &ctx.accounts.obligation,
        &[ctx.accounts.reserve.to_account_info()],
    )?;

    let owner_key = ctx.accounts.owner.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        KaminoAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[ctx.accounts.adapter_position.authority_bump],
    ]];

    // CPI 3: withdraw_obligation_collateral_and_redeem_reserve_collateral.
    // Kamino interprets u64::MAX as "withdraw all" for collateral_amount.
    let collateral_amount = if shares == 0 { u64::MAX } else { ctokens_to_withdraw };

    cpi::cpi_withdraw(
        &ctx.accounts.klend_program,
        &ctx.accounts.kamino_authority,
        &ctx.accounts.obligation,
        &ctx.accounts.lending_market,
        &ctx.accounts.lending_market_authority,
        &ctx.accounts.reserve,
        &ctx.accounts.reserve_liquidity_mint,
        &ctx.accounts.reserve_source_collateral,
        &ctx.accounts.reserve_collateral_mint,
        &ctx.accounts.reserve_liquidity_supply,
        &ctx.accounts.user_destination_liquidity,
        &ctx.accounts.token_program,
        &ctx.accounts.instruction_sysvar,
        collateral_amount,
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos.shares
        .checked_sub(ctokens_to_withdraw)
        .ok_or(error!(AdapterError::InsufficientShares))?;

    set_return_data(&ctokens_to_withdraw.to_le_bytes());
    Ok(())
}
