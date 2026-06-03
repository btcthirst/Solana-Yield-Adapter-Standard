use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{get_return_data, invoke, set_return_data},
};
use registry::state::RegistryEntry;
use crate::state::UserPosition;
use crate::error::DispatcherError;
use crate::cpi_utils::adapter_discriminator;

#[derive(Accounts)]
pub struct CurrentValue<'info> {
    pub owner: Signer<'info>,

    #[account(
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
    // remaining_accounts: read-only adapter-specific accounts for value computation
}

pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let adapter_id = ctx.accounts.adapter_program.key();

    let data = adapter_discriminator("current_value").to_vec();

    let account_metas: Vec<AccountMeta> = ctx.remaining_accounts
        .iter()
        .map(|a| AccountMeta { pubkey: *a.key, is_signer: a.is_signer, is_writable: a.is_writable })
        .collect();

    invoke(
        &Instruction { program_id: adapter_id, accounts: account_metas, data },
        ctx.remaining_accounts,
    )?;

    // Re-propagate adapter's return_data so client reads it from Dispatcher's returnData
    let (_, return_bytes) = get_return_data().ok_or(error!(DispatcherError::AdapterError))?;
    set_return_data(&return_bytes);

    Ok(())
}
