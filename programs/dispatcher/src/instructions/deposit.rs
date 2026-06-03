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
pub struct Deposit<'info> {
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
    // remaining_accounts: adapter-specific accounts in the order the adapter expects
}

pub fn handler(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    let adapter_id = ctx.accounts.adapter_program.key();

    let mut data = adapter_discriminator("deposit").to_vec();
    data.extend_from_slice(&amount.to_le_bytes());

    let account_metas: Vec<AccountMeta> = ctx.remaining_accounts
        .iter()
        .map(|a| AccountMeta { pubkey: *a.key, is_signer: a.is_signer, is_writable: a.is_writable })
        .collect();

    invoke(
        &Instruction { program_id: adapter_id, accounts: account_metas, data },
        ctx.remaining_accounts,
    )?;

    // Adapter must call set_return_data(&shares_received.to_le_bytes())
    let (_, return_bytes) = get_return_data().ok_or(error!(DispatcherError::AdapterError))?;
    require!(return_bytes.len() >= 8, DispatcherError::AdapterError);

    let shares_received = u64::from_le_bytes(
        return_bytes[..8].try_into().map_err(|_| error!(DispatcherError::Overflow))?,
    );

    let pos = &mut ctx.accounts.user_position;
    pos.shares = pos.shares
        .checked_add(shares_received)
        .ok_or(error!(DispatcherError::Overflow))?;

    Ok(())
}
