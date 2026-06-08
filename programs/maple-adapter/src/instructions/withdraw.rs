use anchor_lang::{prelude::*, solana_program::program::set_return_data};
use anchor_spl::token::ID as TOKEN_PROGRAM_ID;

use crate::{cpi, error::AdapterError, state::MapleAdapterPosition};

/// Accounts for `withdraw`.
///
/// Swap `shares` syrupUSDC -> USDC on the Orca whirlpool, then forward the USDC to
/// the user. `shares == 0` withdraws the full position.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// CHECK: position owner; used for PDA seed derivation only.
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [MapleAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MapleAdapterPosition>,

    /// Virtual authority PDA — owns the custody ATAs and signs the swap + payout.
    /// CHECK: PDA derivation validated by seeds constraint.
    #[account(
        seeds = [MapleAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub maple_authority: UncheckedAccount<'info>,

    /// Authority's syrupUSDC ATA — swap input / custody (token_owner_account_a).
    /// CHECK: SPL token account owned by maple_authority.
    #[account(mut)]
    pub authority_syrup_ata: UncheckedAccount<'info>,

    /// Authority's USDC ATA — swap output (token_owner_account_b).
    /// CHECK: SPL token account owned by maple_authority.
    #[account(mut)]
    pub authority_usdc_ata: UncheckedAccount<'info>,

    /// User's USDC ATA — final destination of the withdrawal.
    /// CHECK: SPL token account; validated by the token program.
    #[account(mut)]
    pub user_usdc_ata: UncheckedAccount<'info>,

    /// Orca whirlpool (syrupUSDC/USDC). Read for price; written by the swap.
    /// CHECK: pinned to the canonical whirlpool; contents validated by Orca.
    #[account(mut, address = cpi::SYRUP_WHIRLPOOL @ AdapterError::InvalidPoolState)]
    pub whirlpool: UncheckedAccount<'info>,

    /// CHECK: token A mint (syrupUSDC), pinned.
    #[account(address = cpi::SYRUP_USDC_MINT)]
    pub token_mint_a: UncheckedAccount<'info>,

    /// CHECK: token B mint (USDC), pinned.
    #[account(address = cpi::USDC_MINT)]
    pub token_mint_b: UncheckedAccount<'info>,

    /// CHECK: whirlpool vault A (syrupUSDC), pinned.
    #[account(mut, address = cpi::WHIRLPOOL_VAULT_A)]
    pub token_vault_a: UncheckedAccount<'info>,

    /// CHECK: whirlpool vault B (USDC), pinned.
    #[account(mut, address = cpi::WHIRLPOOL_VAULT_B)]
    pub token_vault_b: UncheckedAccount<'info>,

    /// CHECK: tick array; validated by Orca.
    #[account(mut)]
    pub tick_array_0: UncheckedAccount<'info>,
    /// CHECK: tick array; validated by Orca.
    #[account(mut)]
    pub tick_array_1: UncheckedAccount<'info>,
    /// CHECK: tick array; validated by Orca.
    #[account(mut)]
    pub tick_array_2: UncheckedAccount<'info>,

    /// CHECK: whirlpool oracle PDA, pinned; writable in swap_v2.
    #[account(mut, address = cpi::WHIRLPOOL_ORACLE)]
    pub oracle: UncheckedAccount<'info>,

    /// CHECK: Orca whirlpool program, pinned.
    #[account(address = cpi::WHIRLPOOL_PROGRAM)]
    pub whirlpool_program: UncheckedAccount<'info>,

    /// CHECK: SPL Token program (used for both swap token programs and the payout).
    #[account(address = TOKEN_PROGRAM_ID)]
    pub token_program: UncheckedAccount<'info>,

    /// CHECK: SPL Memo program, pinned (required by swap_v2; no memo emitted).
    #[account(address = cpi::MEMO_PROGRAM)]
    pub memo_program: UncheckedAccount<'info>,
}

/// Withdraw `shares` syrupUSDC lamports: swap to USDC and pay the user.
/// `shares == 0` withdraws the full position. Returns `shares_removed` via
/// `set_return_data`.
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.adapter_position.shares;
    let shares_to_remove = if shares == 0 { pos_shares } else { shares };

    require!(shares_to_remove > 0, AdapterError::InsufficientFunds);
    require!(
        shares_to_remove <= pos_shares,
        AdapterError::InsufficientShares
    );

    // Derive an on-chain min USDC-out from the current pool price.
    let sqrt_price = cpi::read_sqrt_price(&ctx.accounts.whirlpool)?;
    let expected =
        cpi::syrup_to_usdc(shares_to_remove, sqrt_price).ok_or(error!(AdapterError::Overflow))?;
    let min_out = cpi::apply_slippage(expected, cpi::MAX_SLIPPAGE_BPS);
    require!(min_out > 0, AdapterError::SlippageExceeded);

    let owner_key = ctx.accounts.owner.key();
    let auth_bump = ctx.accounts.adapter_position.authority_bump;
    let signer_seeds: &[&[&[u8]]] = &[&[
        MapleAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[auth_bump],
    ]];

    // Swap syrupUSDC -> USDC (A -> B, a_to_b = true) signed by the authority PDA.
    let usdc_before = cpi::read_token_amount(&ctx.accounts.authority_usdc_ata)?;

    let tp = ctx.accounts.token_program.to_account_info();
    let ordered = [
        tp.clone(),                                          // 0 token_program_a
        tp,                                                  // 1 token_program_b
        ctx.accounts.memo_program.to_account_info(),         // 2 memo_program
        ctx.accounts.maple_authority.to_account_info(),      // 3 token_authority
        ctx.accounts.whirlpool.to_account_info(),            // 4 whirlpool
        ctx.accounts.token_mint_a.to_account_info(),         // 5 mint A
        ctx.accounts.token_mint_b.to_account_info(),         // 6 mint B
        ctx.accounts.authority_syrup_ata.to_account_info(),  // 7 owner A
        ctx.accounts.token_vault_a.to_account_info(),        // 8 vault A
        ctx.accounts.authority_usdc_ata.to_account_info(),   // 9 owner B
        ctx.accounts.token_vault_b.to_account_info(),        // 10 vault B
        ctx.accounts.tick_array_0.to_account_info(),         // 11
        ctx.accounts.tick_array_1.to_account_info(),         // 12
        ctx.accounts.tick_array_2.to_account_info(),         // 13
        ctx.accounts.oracle.to_account_info(),               // 14
    ];

    cpi::whirlpool_swap_v2(
        &ctx.accounts.whirlpool_program,
        &ordered,
        shares_to_remove,        // exact syrupUSDC input
        min_out,                 // min USDC out (slippage floor)
        cpi::MIN_SQRT_PRICE,     // price falls for A->B; bound by min_out instead
        true,                    // amount_specified_is_input
        true,                    // a_to_b = true (syrupUSDC -> USDC)
        signer_seeds,
    )?;

    // Forward the USDC actually received to the user.
    let usdc_after = cpi::read_token_amount(&ctx.accounts.authority_usdc_ata)?;
    let usdc_out = usdc_after
        .checked_sub(usdc_before)
        .ok_or(error!(AdapterError::ProtocolError))?;
    require!(usdc_out > 0, AdapterError::SlippageExceeded);

    cpi::cpi_token_transfer(
        &ctx.accounts.token_program,
        &ctx.accounts.authority_usdc_ata,
        &ctx.accounts.user_usdc_ata,
        &ctx.accounts.maple_authority,
        usdc_out,
        signer_seeds,
    )?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos
        .shares
        .checked_sub(shares_to_remove)
        .ok_or(error!(AdapterError::InsufficientShares))?;

    set_return_data(&shares_to_remove.to_le_bytes());
    Ok(())
}
