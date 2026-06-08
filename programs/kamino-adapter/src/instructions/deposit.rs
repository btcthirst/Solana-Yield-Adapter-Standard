use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{
    cpi::{self, FARMS_PROGRAM_ID, INSTRUCTIONS_SYSVAR_ID, KLEND_PROGRAM_ID, MAIN_LENDING_MARKET},
    error::AdapterError,
    state::KaminoAdapterPosition,
};

/// Deposit accounts.
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
pub struct Deposit<'info> {
    /// CHECK: position owner; used only for PDA seed derivation
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [KaminoAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, KaminoAdapterPosition>,

    /// CHECK: PDA derivation validated by seeds constraint.
    /// `mut` because Kamino's deposit requires `owner` to be writable.
    #[account(
        mut,
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

    /// CHECK: Reserve's USDC vault (mut — receives USDC)
    #[account(mut)]
    pub reserve_liquidity_supply: UncheckedAccount<'info>,

    /// CHECK: kUSDC collateral mint (mut — mints ctokens)
    #[account(mut)]
    pub reserve_collateral_mint: UncheckedAccount<'info>,

    /// CHECK: Reserve's collateral supply vault (mut — stores ctokens for obligation)
    #[account(mut)]
    pub reserve_destination_deposit_collateral: UncheckedAccount<'info>,

    /// CHECK: User's USDC ATA (source of funds)
    #[account(mut)]
    pub user_source_liquidity: UncheckedAccount<'info>,

    /// CHECK: SPL Token program
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_SYSVAR_ID)]
    pub instruction_sysvar: UncheckedAccount<'info>,

    /// Obligation farm-user-state (created in initialize_position).
    /// CHECK: validated by the Kamino farms program
    #[account(mut)]
    pub obligation_farm_user_state: UncheckedAccount<'info>,

    /// Reserve collateral farm state (reserve.farmCollateral).
    /// CHECK: validated by the Kamino farms program against the reserve
    #[account(mut)]
    pub reserve_farm_state: UncheckedAccount<'info>,

    /// CHECK: must equal FARMS_PROGRAM_ID
    #[account(address = FARMS_PROGRAM_ID)]
    pub farms_program: UncheckedAccount<'info>,

    /// CHECK: must equal KLEND_PROGRAM_ID
    #[account(address = KLEND_PROGRAM_ID)]
    pub klend_program: UncheckedAccount<'info>,
}

pub fn handler<'info>(ctx: Context<'info, Deposit<'info>>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::InsufficientFunds);

    // Read exchange data before deposit to compute ctokens received.
    let (total_liquidity, ctoken_supply) = {
        let data = ctx.accounts.reserve.try_borrow_data()?;
        cpi::read_reserve_exchange_data(&data)?
    };

    let ctokens_received = cpi::amount_to_ctokens(amount, total_liquidity, ctoken_supply)
        .ok_or(error!(AdapterError::Overflow))?;

    // Build oracle accounts from remaining_accounts[0..4].
    // Use klend_program as placeholder for missing oracle slots.
    let rem = ctx.remaining_accounts;
    let oracle0 = rem.get(0).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());
    let oracle1 = rem.get(1).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());
    let oracle2 = rem.get(2).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());
    let oracle3 = rem.get(3).cloned().unwrap_or_else(|| ctx.accounts.klend_program.to_account_info());

    // CPI 1: refresh_reserve (must precede deposit).
    cpi::cpi_refresh_reserve(
        &ctx.accounts.klend_program,
        &ctx.accounts.reserve,
        &ctx.accounts.lending_market,
        [oracle0, oracle1, oracle2, oracle3],
    )?;

    // CPI 2: refresh_obligation (V2 deposit requires a fresh obligation).
    // refresh_obligation must be passed exactly the reserves the obligation
    // currently holds. For a single-reserve adapter, shares == 0 means the
    // obligation has no USDC deposit yet (Kamino drops empty deposits on full
    // withdraw), so pass no reserves on the first deposit and [reserve] after.
    let reserve_ai = ctx.accounts.reserve.to_account_info();
    let obligation_reserves: Vec<AccountInfo<'info>> =
        if ctx.accounts.adapter_position.shares == 0 {
            vec![]
        } else {
            vec![reserve_ai]
        };
    cpi::cpi_refresh_obligation(
        &ctx.accounts.klend_program,
        &ctx.accounts.lending_market,
        &ctx.accounts.obligation,
        &obligation_reserves,
    )?;

    let owner_key = ctx.accounts.owner.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        KaminoAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[ctx.accounts.adapter_position.authority_bump],
    ]];

    // CPI 2: deposit_reserve_liquidity_and_obligation_collateral.
    cpi::cpi_deposit(
        &ctx.accounts.klend_program,
        &ctx.accounts.kamino_authority,
        &ctx.accounts.obligation,
        &ctx.accounts.lending_market,
        &ctx.accounts.lending_market_authority,
        &ctx.accounts.reserve,
        &ctx.accounts.reserve_liquidity_mint,
        &ctx.accounts.reserve_liquidity_supply,
        &ctx.accounts.reserve_collateral_mint,
        &ctx.accounts.reserve_destination_deposit_collateral,
        &ctx.accounts.user_source_liquidity,
        &ctx.accounts.token_program,
        &ctx.accounts.instruction_sysvar,
        &ctx.accounts.obligation_farm_user_state,
        &ctx.accounts.reserve_farm_state,
        &ctx.accounts.farms_program,
        amount,
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos.shares
        .checked_add(ctokens_received)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&ctokens_received.to_le_bytes());
    Ok(())
}
