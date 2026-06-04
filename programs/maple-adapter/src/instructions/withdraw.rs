use anchor_lang::{prelude::*, solana_program::program::set_return_data};

use crate::{cpi, error::AdapterError, state::MapleAdapterPosition};

/// Accounts for `withdraw`.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// CHECK: position owner; used for PDA seed derivation only
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [MapleAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MapleAdapterPosition>,

    /// Virtual authority PDA — signs the transfer out of its custody ATA.
    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [MapleAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub maple_authority: UncheckedAccount<'info>,

    /// Authority's syrupUSDC ATA (source of withdrawal).
    /// CHECK: validated as authority's ATA; balance enforced by token program
    #[account(mut)]
    pub authority_syrup_ata: UncheckedAccount<'info>,

    /// User's syrupUSDC ATA (receives withdrawn tokens).
    /// CHECK: destination; validated by token program
    #[account(mut)]
    pub user_syrup_ata: UncheckedAccount<'info>,

    /// CHECK: must be SPL Token program
    pub token_program: UncheckedAccount<'info>,
}

/// Withdraw `shares` syrupUSDC lamports from custody back to the user.
/// `shares == 0` withdraws the full position.
///
/// Returns `shares_removed` via `set_return_data`.
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.adapter_position.shares;
    let shares_to_remove = if shares == 0 { pos_shares } else { shares };

    require!(shares_to_remove > 0, AdapterError::InsufficientFunds);
    require!(
        shares_to_remove <= pos_shares,
        AdapterError::InsufficientShares
    );

    let owner_key = ctx.accounts.owner.key();
    let auth_bump = ctx.accounts.adapter_position.authority_bump;
    let signer_seeds: &[&[&[u8]]] = &[&[
        MapleAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[auth_bump],
    ]];

    // Transfer syrupUSDC: authority_syrup_ata → user_syrup_ata.
    // Authority PDA signs via invoke_signed.
    cpi::cpi_token_transfer(
        &ctx.accounts.token_program,
        &ctx.accounts.authority_syrup_ata,
        &ctx.accounts.user_syrup_ata,
        &ctx.accounts.maple_authority,
        shares_to_remove,
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
