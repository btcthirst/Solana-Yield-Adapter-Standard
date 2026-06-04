use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    },
};

/// Jupiter Perpetuals program ID (mainnet).
pub const PERP_PROGRAM_ID: Pubkey = pubkey!("PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu");

/// Jupiter Liquidity Pool (JLP) account.
pub const JLP_POOL: Pubkey = pubkey!("5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAcBn4bE7VF");

/// JLP token mint (6 decimals).
pub const JLP_MINT: Pubkey = pubkey!("27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4");

/// USDC mint (mainnet, 6 decimals).
pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

/// USDC custody account in the JLP pool.
pub const USDC_CUSTODY: Pubkey = pubkey!("G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa");

// ── Instruction discriminators ─────────────────────────────────────────────────
// sha256("global:{name}")[0..8]

/// sha256("global:add_liquidity")[0..8]
pub const DISC_ADD_LIQUIDITY: [u8; 8] = [0xb5, 0x9d, 0x59, 0x43, 0x8f, 0xb6, 0x34, 0x48];
/// sha256("global:remove_liquidity")[0..8]
pub const DISC_REMOVE_LIQUIDITY: [u8; 8] = [0x50, 0x55, 0xd1, 0x48, 0x18, 0xce, 0xb1, 0x6c];

// ── Pool account layout helpers ────────────────────────────────────────────────
//
// Jupiter Perpetuals Pool struct (Anchor account):
//   [0..8]           discriminator
//   [8..8+4+n]       name: String  (4-byte length prefix + n bytes)
//   [+4+32*c]        custodies: Vec<Pubkey>  (4-byte length + 32 bytes each)
//   [+4+24*c]        ratios: Vec<TokenRatios> (4-byte length + 24 bytes each)
//                        TokenRatios { target: u64, min: u64, max: u64 } = 24 bytes
//   [+16]            aum_usd: u128  ← total pool USD value (6-decimal precision)
//   ...              remaining fields
//
// aum_usd uses USD_DECIMALS = 6, so 1 USD = 1_000_000 units (USDC lamports equivalent).

/// Read `aum_usd` (u128) from raw Pool account data by walking variable-length fields.
pub fn read_pool_aum(pool_data: &[u8]) -> Result<u128> {
    require!(pool_data.len() > 8, crate::error::AdapterError::PoolDataTooShort);

    let mut cursor = 8usize; // skip discriminator

    // name: String (4-byte len + bytes)
    require!(pool_data.len() >= cursor + 4, crate::error::AdapterError::PoolDataTooShort);
    let name_len = u32::from_le_bytes(pool_data[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4 + name_len;

    // custodies: Vec<Pubkey> (4-byte count + 32 bytes each)
    require!(pool_data.len() >= cursor + 4, crate::error::AdapterError::PoolDataTooShort);
    let custodies_count = u32::from_le_bytes(pool_data[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4 + custodies_count * 32;

    // ratios: Vec<TokenRatios> (4-byte count + 24 bytes each)
    require!(pool_data.len() >= cursor + 4, crate::error::AdapterError::PoolDataTooShort);
    let ratios_count = u32::from_le_bytes(pool_data[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4 + ratios_count * 24;

    // aum_usd: u128
    require!(
        pool_data.len() >= cursor + 16,
        crate::error::AdapterError::PoolDataTooShort
    );
    let aum_usd = u128::from_le_bytes(pool_data[cursor..cursor + 16].try_into().unwrap());

    Ok(aum_usd)
}

/// Read supply (u64) from raw SPL token Mint account data.
///
/// Mint layout:
///   [0..4]   COption tag for mint_authority
///   [4..36]  Pubkey (mint_authority)
///   [36..44] u64  supply  ← here
pub fn read_mint_supply(mint_data: &[u8]) -> Result<u64> {
    require!(mint_data.len() >= 44, crate::error::AdapterError::PoolDataTooShort);
    Ok(u64::from_le_bytes(mint_data[36..44].try_into().unwrap()))
}

/// Compute current USDC value of `shares` JLP tokens.
///
/// value = shares * aum_usd / jlp_supply
///
/// All quantities use 6-decimal precision, so the result is in USDC lamports.
pub fn jlp_to_usdc(shares: u64, aum_usd: u128, jlp_supply: u64) -> Option<u64> {
    if jlp_supply == 0 {
        return Some(0);
    }
    let value = (shares as u128)
        .checked_mul(aum_usd)?
        .checked_div(jlp_supply as u128)?;
    u64::try_from(value).ok()
}

// ── CPI helpers ───────────────────────────────────────────────────────────────

/// CPI: Jupiter Perpetuals `add_liquidity`.
///
/// Accounts:
///   0.  owner (signer, mut) — jlp_authority PDA
///   1.  fundingAccount (mut) — authority's USDC ATA (source)
///   2.  lpTokenAccount (mut) — authority's JLP ATA (destination)
///   3.  transferAuthority — PDA [b"transfer_authority"] @ PERP
///   4.  perpetuals (mut) — PDA [b"perpetuals"] @ PERP
///   5.  pool (mut) — JLP pool
///   6.  custody (mut) — USDC custody
///   7.  custodyOracleAccount — price oracle for USDC custody
///   8.  custodyTokenAccount (mut) — USDC vault (custody token account)
///   9.  lpTokenMint (mut) — JLP token mint
///   10. tokenProgram
///
/// Args: { amount_in: u64, min_lp_amount_out: u64 }
#[allow(clippy::too_many_arguments)]
pub fn cpi_add_liquidity<'info>(
    perp_program: &AccountInfo<'info>,
    owner: &AccountInfo<'info>,               // jlp_authority PDA
    funding_account: &AccountInfo<'info>,     // authority USDC ATA
    lp_token_account: &AccountInfo<'info>,    // authority JLP ATA
    transfer_authority: &AccountInfo<'info>,
    perpetuals: &AccountInfo<'info>,
    pool: &AccountInfo<'info>,
    custody: &AccountInfo<'info>,
    custody_oracle: &AccountInfo<'info>,
    custody_token_account: &AccountInfo<'info>,
    lp_token_mint: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    amount_in: u64,
    min_lp_amount_out: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_ADD_LIQUIDITY.to_vec();
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_lp_amount_out.to_le_bytes());

    invoke_signed(
        &Instruction {
            program_id: perp_program.key(),
            accounts: vec![
                AccountMeta::new(owner.key(), true),
                AccountMeta::new(funding_account.key(), false),
                AccountMeta::new(lp_token_account.key(), false),
                AccountMeta::new_readonly(transfer_authority.key(), false),
                AccountMeta::new(perpetuals.key(), false),
                AccountMeta::new(pool.key(), false),
                AccountMeta::new(custody.key(), false),
                AccountMeta::new_readonly(custody_oracle.key(), false),
                AccountMeta::new(custody_token_account.key(), false),
                AccountMeta::new(lp_token_mint.key(), false),
                AccountMeta::new_readonly(token_program.key(), false),
            ],
            data,
        },
        &[
            owner.clone(),
            funding_account.clone(),
            lp_token_account.clone(),
            transfer_authority.clone(),
            perpetuals.clone(),
            pool.clone(),
            custody.clone(),
            custody_oracle.clone(),
            custody_token_account.clone(),
            lp_token_mint.clone(),
            token_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}

/// CPI: Jupiter Perpetuals `remove_liquidity`.
///
/// Accounts:
///   0.  owner (signer) — jlp_authority PDA
///   1.  receivingAccount (mut) — user's USDC ATA (destination)
///   2.  lpTokenAccount (mut) — authority's JLP ATA (source, JLP burned here)
///   3.  transferAuthority — PDA [b"transfer_authority"] @ PERP
///   4.  perpetuals (mut) — PDA [b"perpetuals"] @ PERP
///   5.  pool (mut) — JLP pool
///   6.  custody (mut) — USDC custody
///   7.  custodyOracleAccount — price oracle for USDC custody
///   8.  custodyTokenAccount (mut) — USDC vault
///   9.  lpTokenMint (mut) — JLP token mint
///   10. tokenProgram
///
/// Args: { lp_amount_in: u64, min_amount_out: u64 }
#[allow(clippy::too_many_arguments)]
pub fn cpi_remove_liquidity<'info>(
    perp_program: &AccountInfo<'info>,
    owner: &AccountInfo<'info>,               // jlp_authority PDA
    receiving_account: &AccountInfo<'info>,   // user USDC ATA
    lp_token_account: &AccountInfo<'info>,    // authority JLP ATA
    transfer_authority: &AccountInfo<'info>,
    perpetuals: &AccountInfo<'info>,
    pool: &AccountInfo<'info>,
    custody: &AccountInfo<'info>,
    custody_oracle: &AccountInfo<'info>,
    custody_token_account: &AccountInfo<'info>,
    lp_token_mint: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    lp_amount_in: u64,
    min_amount_out: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_REMOVE_LIQUIDITY.to_vec();
    data.extend_from_slice(&lp_amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());

    invoke_signed(
        &Instruction {
            program_id: perp_program.key(),
            accounts: vec![
                AccountMeta::new(owner.key(), true),
                AccountMeta::new(receiving_account.key(), false),
                AccountMeta::new(lp_token_account.key(), false),
                AccountMeta::new_readonly(transfer_authority.key(), false),
                AccountMeta::new(perpetuals.key(), false),
                AccountMeta::new(pool.key(), false),
                AccountMeta::new(custody.key(), false),
                AccountMeta::new_readonly(custody_oracle.key(), false),
                AccountMeta::new(custody_token_account.key(), false),
                AccountMeta::new(lp_token_mint.key(), false),
                AccountMeta::new_readonly(token_program.key(), false),
            ],
            data,
        },
        &[
            owner.clone(),
            receiving_account.clone(),
            lp_token_account.clone(),
            transfer_authority.clone(),
            perpetuals.clone(),
            pool.clone(),
            custody.clone(),
            custody_oracle.clone(),
            custody_token_account.clone(),
            lp_token_mint.clone(),
            token_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}
