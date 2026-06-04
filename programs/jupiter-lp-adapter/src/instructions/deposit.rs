use anchor_lang::{
    prelude::*,
    solana_program::program::{invoke, set_return_data},
};
use anchor_spl::token::{Token, TokenAccount};

use crate::{
    cpi::{self, JLP_MINT, JLP_POOL, PERP_PROGRAM_ID, USDC_CUSTODY, USDC_MINT},
    error::AdapterError,
    state::JupiterLpAdapterPosition,
};

/// Deposit accounts.
///
/// Called indirectly via Dispatcher CPI (invoke), so `owner` is not a Signer
/// in the Anchor context — but the signer bit IS set on the AccountInfo because
/// the user signed the outer transaction and Dispatcher calls this via `invoke`.
///
/// Remaining accounts:
///   [0] custody_oracle — price oracle for the USDC custody (from custody.oracle)
#[derive(Accounts)]
pub struct Deposit<'info> {
    /// CHECK: position owner; used for PDA seeds and as USDC transfer authority
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [JupiterLpAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, JupiterLpAdapterPosition>,

    /// CHECK: PDA derivation validated by seeds constraint
    #[account(
        seeds = [JupiterLpAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub jlp_authority: UncheckedAccount<'info>,

    /// CHECK: validated by address constraint
    #[account(address = PERP_PROGRAM_ID)]
    pub perp_program: UncheckedAccount<'info>,

    /// CHECK: validated by address constraint
    #[account(address = JLP_POOL)]
    pub pool: UncheckedAccount<'info>,

    /// CHECK: validated by address constraint
    #[account(address = USDC_CUSTODY)]
    pub custody: UncheckedAccount<'info>,

    /// CHECK: USDC custody vault; seeds [b"custody_token_account", pool, usdc_mint] @ PERP
    #[account(
        mut,
        seeds = [b"custody_token_account", JLP_POOL.as_ref(), USDC_MINT.as_ref()],
        bump,
        seeds::program = PERP_PROGRAM_ID,
    )]
    pub custody_token_account: UncheckedAccount<'info>,

    /// CHECK: Jupiter Perpetuals transferAuthority PDA
    #[account(
        seeds = [b"transfer_authority"],
        bump,
        seeds::program = PERP_PROGRAM_ID,
    )]
    pub transfer_authority: UncheckedAccount<'info>,

    /// CHECK: Jupiter Perpetuals state PDA
    #[account(
        mut,
        seeds = [b"perpetuals"],
        bump,
        seeds::program = PERP_PROGRAM_ID,
    )]
    pub perpetuals: UncheckedAccount<'info>,

    /// CHECK: JLP token mint; validated by address constraint
    #[account(mut, address = JLP_MINT)]
    pub jlp_mint: UncheckedAccount<'info>,

    /// Authority's JLP ATA — receives JLP from addLiquidity.
    /// Constrained to be the canonical ATA of jlp_authority for JLP_MINT.
    #[account(
        mut,
        associated_token::mint = jlp_mint,
        associated_token::authority = jlp_authority,
    )]
    pub authority_jlp_ata: Account<'info, TokenAccount>,

    /// Authority's USDC ATA — staging account for the USDC pre-transfer.
    /// Constrained to be the canonical ATA of jlp_authority for USDC_MINT.
    #[account(
        mut,
        associated_token::mint = usdc_mint_account,
        associated_token::authority = jlp_authority,
    )]
    pub authority_usdc_ata: Account<'info, TokenAccount>,

    /// User's USDC ATA — USDC is transferred from here into authority_usdc_ata.
    /// CHECK: caller-provided; ownership validated by SPL Token CPI
    #[account(mut)]
    pub user_usdc_ata: UncheckedAccount<'info>,

    /// CHECK: USDC mint for ATA derivation
    #[account(address = USDC_MINT)]
    pub usdc_mint_account: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, anchor_spl::associated_token::AssociatedToken>,
}

pub fn handler<'info>(ctx: Context<'info, Deposit<'info>>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::InsufficientFunds);

    let owner_key = ctx.accounts.owner.key();
    let auth_bump = ctx.accounts.adapter_position.authority_bump;
    let signer_seeds: &[&[&[u8]]] = &[&[
        JupiterLpAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[auth_bump],
    ]];

    // Step 1: transfer USDC from user's ATA → authority's USDC ATA.
    // The user's signer bit propagates through Dispatcher's invoke chain.
    invoke(
        &spl_token_transfer_ix(
            ctx.accounts.token_program.to_account_info().key,
            ctx.accounts.user_usdc_ata.key,
            &ctx.accounts.authority_usdc_ata.key(),
            ctx.accounts.owner.key,
            amount,
        )?,
        &[
            ctx.accounts.user_usdc_ata.to_account_info(),
            ctx.accounts.authority_usdc_ata.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        ],
    )?;

    // Step 2: JLP balance before addLiquidity to compute delta.
    let jlp_before = ctx.accounts.authority_jlp_ata.amount;

    // Step 3: read oracle from remaining_accounts[0], fall back to perp_program as placeholder.
    let oracle = ctx
        .remaining_accounts
        .first()
        .cloned()
        .unwrap_or_else(|| ctx.accounts.perp_program.to_account_info());

    // Step 4: CPI addLiquidity — authority signs, USDC from authority ATA, JLP to authority ATA.
    cpi::cpi_add_liquidity(
        &ctx.accounts.perp_program,
        &ctx.accounts.jlp_authority,
        &ctx.accounts.authority_usdc_ata.to_account_info(),
        &ctx.accounts.authority_jlp_ata.to_account_info(),
        &ctx.accounts.transfer_authority,
        &ctx.accounts.perpetuals,
        &ctx.accounts.pool,
        &ctx.accounts.custody,
        &oracle,
        &ctx.accounts.custody_token_account,
        &ctx.accounts.jlp_mint,
        &ctx.accounts.token_program,
        amount,
        0, // no slippage protection in the reference adapter
        signer_seeds,
    )?;

    // Step 5: compute JLP tokens received and update shares.
    // Reload account data after CPI to see the updated balance.
    ctx.accounts.authority_jlp_ata.reload()?;
    let jlp_after = ctx.accounts.authority_jlp_ata.amount;
    let jlp_received = jlp_after
        .checked_sub(jlp_before)
        .ok_or(error!(AdapterError::ProtocolError))?;

    require!(jlp_received > 0, AdapterError::ProtocolError);

    let pos = &mut ctx.accounts.adapter_position;
    pos.shares = pos
        .shares
        .checked_add(jlp_received)
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&jlp_received.to_le_bytes());
    Ok(())
}

// ── helper ───────────────────────────────────────────────────────────────────

fn spl_token_transfer_ix(
    token_program_id: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Result<anchor_lang::solana_program::instruction::Instruction> {
    // SPL Token transfer discriminator = 3u8
    let mut data = vec![3u8];
    data.extend_from_slice(&amount.to_le_bytes());
    Ok(anchor_lang::solana_program::instruction::Instruction {
        program_id: *token_program_id,
        accounts: vec![
            anchor_lang::solana_program::instruction::AccountMeta::new(*source, false),
            anchor_lang::solana_program::instruction::AccountMeta::new(*destination, false),
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(*authority, true),
        ],
        data,
    })
}

