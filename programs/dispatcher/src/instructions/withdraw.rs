use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{get_return_data, invoke},
};
use registry::state::RegistryEntry;
use crate::state::UserPosition;
use crate::error::DispatcherError;
use crate::cpi_utils::adapter_discriminator;

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        seeds = [UserPosition::SEED, owner.key().as_ref(), adapter_program.key().as_ref()],
        bump = user_position.bump,
        constraint = user_position.owner == owner.key(),
        constraint = user_position.adapter == adapter_program.key(),
    )]
    pub user_position: Account<'info, UserPosition>,

    #[account(
        seeds = [RegistryEntry::SEED, adapter_program.key().as_ref()],
        bump = registry_entry.bump,
        seeds::program = registry::ID,
        constraint = registry_entry.is_active @ DispatcherError::AdapterRevoked,
        constraint = !registry_entry.is_paused @ DispatcherError::AdapterPaused,
    )]
    pub registry_entry: Account<'info, RegistryEntry>,

    /// CHECK: adapter program — validated via registry_entry PDA derivation (seeds::program)
    pub adapter_program: UncheckedAccount<'info>,
    // remaining_accounts: adapter-specific accounts in expected order
}

pub fn handler(ctx: Context<Withdraw>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.user_position.shares;

    // shares = 0 means "withdraw all"
    let shares_to_withdraw = if shares == 0 {
        pos_shares
    } else {
        require!(shares <= pos_shares, DispatcherError::InsufficientShares);
        shares
    };

    let adapter_id = ctx.accounts.adapter_program.key();

    let mut data = adapter_discriminator("withdraw").to_vec();
    data.extend_from_slice(&shares_to_withdraw.to_le_bytes());

    let account_metas: Vec<AccountMeta> = ctx.remaining_accounts
        .iter()
        .map(|a| AccountMeta { pubkey: *a.key, is_signer: a.is_signer, is_writable: a.is_writable })
        .collect();

    invoke(
        &Instruction { program_id: adapter_id, accounts: account_metas, data },
        ctx.remaining_accounts,
    )?;

    // Adapter returns shares_removed (0 for cooldown protocols on first call)
    let (_, return_bytes) = get_return_data().ok_or(error!(DispatcherError::AdapterError))?;
    require!(return_bytes.len() >= 8, DispatcherError::AdapterError);

    let shares_removed = u64::from_le_bytes(
        return_bytes[..8].try_into().map_err(|_| error!(DispatcherError::Overflow))?,
    );

    let pos = &mut ctx.accounts.user_position;
    pos.shares = pos.shares
        .checked_sub(shares_removed)
        .ok_or(error!(DispatcherError::Overflow))?;

    Ok(())
}
