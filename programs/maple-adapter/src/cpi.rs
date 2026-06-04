use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};

/// syrupUSDC SPL-token mint on Solana mainnet.
/// Confirmed via Orca pool and Maple Finance docs (June 2025).
pub const SYRUP_USDC_MINT: Pubkey = pubkey!("AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj");

/// Maple pool state account on Solana.
/// Stores NAV data pushed from Ethereum via Chainlink CCIP.
/// Layout (Anchor account, 8-byte discriminator):
///   [0..8]   discriminator
///   [8..16]  total_assets_usdc: u64  (USDC lamports, 6 decimals)
///   [16..24] total_syrup_supply: u64 (syrupUSDC lamports, 6 decimals)
/// NAV = total_assets_usdc / total_syrup_supply, scaled to 1e6
/// NOTE: offsets need on-chain verification against mainnet account data.
pub const MAPLE_POOL_STATE: Pubkey = pubkey!("HrTBpF3LqSxXnjnYdR4htnBLyMHNZ6eNaDZGPundvHbm");

/// Fallback NAV when pool state is unavailable: 1.0 USDC per syrupUSDC.
/// Real value is always >= 1.0 as yield accrues.
pub const NAV_ONE: u64 = 1_000_000; // 1e6

const TOTAL_ASSETS_OFFSET: usize = 8;
const TOTAL_SUPPLY_OFFSET: usize = 16;

/// Read current NAV (USDC lamports per 1 syrupUSDC, scaled by 1e6) from pool state.
/// Falls back to NAV_ONE if the account is absent or data is out of range.
pub fn read_nav(pool_state: &AccountInfo) -> u64 {
    let data = match pool_state.try_borrow_data() {
        Ok(d) => d,
        Err(_) => return NAV_ONE,
    };

    if data.len() < TOTAL_SUPPLY_OFFSET + 8 {
        return NAV_ONE;
    }

    let total_assets = u64::from_le_bytes(
        data[TOTAL_ASSETS_OFFSET..TOTAL_ASSETS_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    let total_supply = u64::from_le_bytes(
        data[TOTAL_SUPPLY_OFFSET..TOTAL_SUPPLY_OFFSET + 8]
            .try_into()
            .unwrap(),
    );

    if total_supply == 0 || total_assets == 0 {
        return NAV_ONE;
    }

    // nav = total_assets * 1e6 / total_supply
    // Both values have 6 decimals, so this gives USDC per syrupUSDC × 1e6.
    let nav = (total_assets as u128)
        .checked_mul(NAV_ONE as u128)
        .and_then(|v| v.checked_div(total_supply as u128))
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(NAV_ONE);

    // Sanity: NAV must be >= 1.0 and < 1000.0 USDC per syrupUSDC.
    if nav < NAV_ONE || nav > 1_000_000_000_000 {
        NAV_ONE
    } else {
        nav
    }
}

/// Convert syrupUSDC shares to USDC value using current NAV.
/// value_usdc = shares * nav / 1e6
pub fn shares_to_usdc(shares: u64, nav: u64) -> Option<u64> {
    let value = (shares as u128)
        .checked_mul(nav as u128)?
        .checked_div(NAV_ONE as u128)?;
    u64::try_from(value).ok()
}

/// Convert USDC amount to estimated syrupUSDC shares using current NAV.
/// shares = usdc * 1e6 / nav
pub fn usdc_to_shares(usdc: u64, nav: u64) -> Option<u64> {
    let shares = (usdc as u128)
        .checked_mul(NAV_ONE as u128)?
        .checked_div(nav as u128)?;
    u64::try_from(shares).ok()
}

// sha256("global:transfer")[0..8] — SPL Token transfer discriminator via raw invoke.
// We use anchor_spl's token::transfer instead; this module exposes the raw CPI helpers
// for the SPL Token program transfer instruction (for UncheckedAccount contexts).

pub const SPL_TOKEN_TRANSFER_DISC: u8 = 3; // SPL Token instruction: Transfer

/// CPI: SPL Token `transfer` — move `amount` tokens from `src` to `dst`.
/// `authority` must be a signer (PDA → invoke_signed).
pub fn cpi_token_transfer<'info>(
    token_program: &AccountInfo<'info>,
    src: &AccountInfo<'info>,
    dst: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = vec![SPL_TOKEN_TRANSFER_DISC];
    data.extend_from_slice(&amount.to_le_bytes());

    invoke_signed(
        &Instruction {
            program_id: token_program.key(),
            accounts: vec![
                AccountMeta::new(src.key(), false),
                AccountMeta::new(dst.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
            ],
            data,
        },
        &[src.clone(), dst.clone(), authority.clone()],
        signer_seeds,
    )?;
    Ok(())
}
