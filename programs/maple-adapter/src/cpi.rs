use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};

use crate::error::AdapterError;

// ─── Mints ────────────────────────────────────────────────────────────────────

/// syrupUSDC SPL-token mint on Solana mainnet (6 decimals).
/// syrupUSDC is Maple's yield-bearing token, bridged to Solana via Chainlink CCIP.
/// There is NO native Maple deposit program on Solana — exposure is obtained by
/// buying/selling syrupUSDC on the secondary market (the Orca whirlpool below).
pub const SYRUP_USDC_MINT: Pubkey = pubkey!("AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj");

/// USDC mint (6 decimals).
pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

// ─── Orca Whirlpool: syrupUSDC (token A) / USDC (token B) ───────────────────────
//
// Verified on mainnet (2026-06): live, actively-traded concentrated-liquidity pool.
//   tickSpacing = 1, fee = 0.01%, price ≈ 1.168 USDC per syrupUSDC (= live NAV).
// All swaps route through `swap_v2` (token-2022-compatible variant); both mints are
// classic SPL so both token programs are the standard SPL Token program.

pub const WHIRLPOOL_PROGRAM: Pubkey = pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
pub const SYRUP_WHIRLPOOL: Pubkey = pubkey!("6fteKNvMdv7tYmBoJHhj1jx6rHcEwC6RdSEmVpyS613J");
/// Whirlpool vault for token A (syrupUSDC).
pub const WHIRLPOOL_VAULT_A: Pubkey = pubkey!("FM2RuqFYo9umA1yc5FyQn6pSDZJZ1MXAdaekJZ4dQCvi");
/// Whirlpool vault for token B (USDC).
pub const WHIRLPOOL_VAULT_B: Pubkey = pubkey!("Fw6Xr45rBBrXbWJd5ZbSg44kacrKRLef4rHkZ8gWC5Ab");
/// Whirlpool oracle PDA: [b"oracle", whirlpool] @ WHIRLPOOL_PROGRAM. Writable in swap_v2.
pub const WHIRLPOOL_ORACLE: Pubkey = pubkey!("H7j5FQpwTUMwxrWeuyrLr5Z9oHsPFiaRqNaERVsuE1c8");
/// SPL Memo program (required account for swap_v2; no memo is emitted).
pub const MEMO_PROGRAM: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

/// Orca whirlpool `swap_v2` discriminator = sha256("global:swap_v2")[0..8].
const SWAP_V2_DISC: [u8; 8] = [0x2b, 0x04, 0xed, 0x0b, 0x1a, 0xc9, 0x1e, 0x62];

/// Orca price bounds (Q64.64 sqrt price). Used as the swap's `sqrt_price_limit`;
/// real slippage protection comes from `other_amount_threshold` (min-out).
pub const MIN_SQRT_PRICE: u128 = 4_295_048_016;
pub const MAX_SQRT_PRICE: u128 = 79_226_673_515_401_279_992_447_579_055;

/// Max slippage tolerated by the in-adapter swap (1.00%). The min-out is derived
/// on-chain from the pool's current price, so the interface stays `deposit(amount)`
/// / `withdraw(shares)` with no extra client-supplied slippage argument.
pub const MAX_SLIPPAGE_BPS: u64 = 100;
const BPS_DENOM: u128 = 10_000;

/// Q64.64 fixed-point one.
const Q64: u128 = 1u128 << 64;

/// Byte offset of `sqrt_price` (u128) inside the Whirlpool account. Verified on mainnet.
const SQRT_PRICE_OFFSET: usize = 65;
/// End offset of `sqrt_price` (SQRT_PRICE_OFFSET + 16).
const SQRT_PRICE_END: usize = 81;
/// Byte offset of `amount` (u64) inside an SPL token account.
const TOKEN_AMOUNT_OFFSET: usize = 64;
/// End offset of the token `amount` field (TOKEN_AMOUNT_OFFSET + 8).
const TOKEN_AMOUNT_END: usize = 72;

// ─── Pool reads ─────────────────────────────────────────────────────────────────

/// Read the whirlpool's current `sqrt_price` (Q64.64). The caller must have pinned
/// `whirlpool` to `SYRUP_WHIRLPOOL` via an address constraint.
pub fn read_sqrt_price(whirlpool: &AccountInfo) -> Result<u128> {
    let data = whirlpool
        .try_borrow_data()
        .map_err(|_| error!(AdapterError::InvalidPoolState))?;
    require!(data.len() >= SQRT_PRICE_END, AdapterError::InvalidPoolState);
    let bytes: [u8; 16] = data[SQRT_PRICE_OFFSET..SQRT_PRICE_END]
        .try_into()
        .map_err(|_| error!(AdapterError::InvalidPoolState))?;
    Ok(u128::from_le_bytes(bytes))
}

/// Read the `amount` field of an SPL token account.
pub fn read_token_amount(token_acct: &AccountInfo) -> Result<u64> {
    let data = token_acct
        .try_borrow_data()
        .map_err(|_| error!(AdapterError::ProtocolError))?;
    require!(data.len() >= TOKEN_AMOUNT_END, AdapterError::ProtocolError);
    let bytes: [u8; 8] = data[TOKEN_AMOUNT_OFFSET..TOKEN_AMOUNT_END]
        .try_into()
        .map_err(|_| error!(AdapterError::ProtocolError))?;
    Ok(u64::from_le_bytes(bytes))
}

// ─── Price math (token A = syrupUSDC, token B = USDC, both 6 decimals) ───────────
//
// price (USDC per syrupUSDC) = (sqrt_price / 2^64)^2.

/// USDC value of `shares` syrupUSDC lamports at the given sqrt price.
/// value = shares * sqrt^2 / 2^128, computed in two shift-down steps to stay in u128.
pub fn syrup_to_usdc(shares: u64, sqrt_price: u128) -> Option<u64> {
    let t = (shares as u128).checked_mul(sqrt_price)?.checked_div(Q64)?;
    let v = t.checked_mul(sqrt_price)?.checked_div(Q64)?;
    u64::try_from(v).ok()
}

/// Estimated syrupUSDC lamports obtainable for `usdc` lamports at the given sqrt price.
/// out = usdc * 2^128 / sqrt^2.
pub fn usdc_to_syrup(usdc: u64, sqrt_price: u128) -> Option<u64> {
    let t = (usdc as u128).checked_mul(Q64)?.checked_div(sqrt_price)?;
    let s = t.checked_mul(Q64)?.checked_div(sqrt_price)?;
    u64::try_from(s).ok()
}

/// Apply a slippage floor: `amount * (10000 - bps) / 10000`.
pub fn apply_slippage(amount: u64, bps: u64) -> u64 {
    let keep = BPS_DENOM.saturating_sub(bps as u128);
    ((amount as u128).saturating_mul(keep) / BPS_DENOM) as u64
}

// ─── Orca swap_v2 CPI ────────────────────────────────────────────────────────────

/// CPI into Orca `swap_v2`. `ordered` must be exactly the 15 accounts in canonical
/// swap_v2 order:
///   [0] token_program_a   [1] token_program_b   [2] memo_program
///   [3] token_authority(signer)  [4] whirlpool
///   [5] token_mint_a      [6] token_mint_b
///   [7] token_owner_account_a    [8] token_vault_a
///   [9] token_owner_account_b    [10] token_vault_b
///   [11] tick_array_0  [12] tick_array_1  [13] tick_array_2  [14] oracle
#[allow(clippy::too_many_arguments)]
pub fn whirlpool_swap_v2<'info>(
    whirlpool_program: &AccountInfo<'info>,
    ordered: &[AccountInfo<'info>],
    amount: u64,
    other_amount_threshold: u64,
    sqrt_price_limit: u128,
    amount_specified_is_input: bool,
    a_to_b: bool,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    require!(ordered.len() == 15, AdapterError::ProtocolError);

    let mut data = SWAP_V2_DISC.to_vec();
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&other_amount_threshold.to_le_bytes());
    data.extend_from_slice(&sqrt_price_limit.to_le_bytes());
    data.push(amount_specified_is_input as u8);
    data.push(a_to_b as u8);
    data.push(0u8); // remaining_accounts_info: Option<..> = None

    // (index, is_writable, is_signer)
    let spec: [(usize, bool, bool); 15] = [
        (0, false, false),  // token_program_a
        (1, false, false),  // token_program_b
        (2, false, false),  // memo_program
        (3, false, true),   // token_authority
        (4, true, false),   // whirlpool
        (5, false, false),  // token_mint_a
        (6, false, false),  // token_mint_b
        (7, true, false),   // token_owner_account_a
        (8, true, false),   // token_vault_a
        (9, true, false),   // token_owner_account_b
        (10, true, false),  // token_vault_b
        (11, true, false),  // tick_array_0
        (12, true, false),  // tick_array_1
        (13, true, false),  // tick_array_2
        (14, true, false),  // oracle
    ];
    let accounts: Vec<AccountMeta> = spec
        .iter()
        .map(|&(i, w, s)| AccountMeta {
            pubkey: ordered[i].key(),
            is_signer: s,
            is_writable: w,
        })
        .collect();

    invoke_signed(
        &Instruction {
            program_id: whirlpool_program.key(),
            accounts,
            data,
        },
        ordered,
        signer_seeds,
    )?;
    Ok(())
}

// ─── SPL Token transfer (raw CPI for UncheckedAccount contexts) ──────────────────

/// SPL Token `Transfer` instruction tag.
pub const SPL_TOKEN_TRANSFER_DISC: u8 = 3;

/// CPI: SPL Token `transfer` — move `amount` tokens from `src` to `dst`.
/// `authority` signs (a user wallet via propagated signature, or a PDA via seeds).
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
