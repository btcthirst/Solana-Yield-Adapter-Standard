use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke,
    },
};

// ── Program / account constants ─────────────────────────────────────────────

pub const DRIFT_PROGRAM_ID: Pubkey = pubkey!("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");
pub const DRIFT_STATE: Pubkey      = pubkey!("5zpq7DvB6UdFFvpmBPspGPNfUGoBRRCE2HHg5u3gxcsN");
pub const DRIFT_SIGNER: Pubkey     = pubkey!("JCNCMFXo5M5qwUPg2Utu1u6YWp3MbygxqBsBeXXJfrw");

/// USDC spot market config account (market_index = 0)
pub const USDC_SPOT_MARKET: Pubkey = pubkey!("6gMq3mRCKf8aP3ttTyYhuijVZ2LGi14oDsBbkgubfLB3");

pub const MARKET_INDEX: u16 = 0;

// ── Instruction discriminators (sha256("global:{snake_name}")[0..8]) ─────────

/// sha256("global:initialize_user_stats")[0..8]
pub const DISC_INIT_USER_STATS: [u8; 8] = [0xfe, 0xf3, 0x48, 0x62, 0xfb, 0x82, 0xa8, 0xd5];
/// sha256("global:initialize_user")[0..8]
pub const DISC_INIT_USER: [u8; 8]       = [0x6f, 0x11, 0xb9, 0xfa, 0x3c, 0x7a, 0x26, 0xfe];
/// sha256("global:deposit")[0..8]
pub const DISC_DEPOSIT: [u8; 8]         = [0xf2, 0x23, 0xc6, 0x89, 0x52, 0xe1, 0xf2, 0xb6];
/// sha256("global:withdraw")[0..8]
pub const DISC_WITHDRAW: [u8; 8]        = [0xb7, 0x12, 0x46, 0x9c, 0x94, 0x6d, 0xa1, 0x22];

// ── CPI functions ────────────────────────────────────────────────────────────

/// CPI: Drift `initialize_user_stats`.
/// Accounts (IDL order):
///   0. user_stats      (mut, init PDA — seeds: ["user_stats", authority])
///   1. state           (mut)
///   2. authority       (signer)
///   3. payer           (mut, signer)
///   4. rent            (sysvar)
///   5. system_program
pub fn cpi_initialize_user_stats<'info>(
    drift_program: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    rent: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
) -> Result<()> {
    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new(user_stats.key(), false),
                AccountMeta::new(state.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new(payer.key(), true),
                AccountMeta::new_readonly(rent.key(), false),
                AccountMeta::new_readonly(system_program.key(), false),
            ],
            data: DISC_INIT_USER_STATS.to_vec(),
        },
        &[
            user_stats.clone(),
            state.clone(),
            authority.clone(),
            payer.clone(),
            rent.clone(),
            system_program.clone(),
        ],
    )?;
    Ok(())
}

/// CPI: Drift `initialize_user`.
/// Creates the user sub-account for spot/perp positions.
/// Accounts (IDL order):
///   0. user            (mut, init PDA — seeds: ["user", authority, sub_account_id_le])
///   1. user_stats      (mut)
///   2. state           (mut)
///   3. authority       (signer)
///   4. payer           (mut, signer)
///   5. rent            (sysvar)
///   6. system_program
/// Data: discriminator(8) + sub_account_id: u16 LE + name: [u8; 32]
pub fn cpi_initialize_user<'info>(
    drift_program: &AccountInfo<'info>,
    user: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    rent: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    sub_account_id: u16,
) -> Result<()> {
    let mut data = DISC_INIT_USER.to_vec();
    data.extend_from_slice(&sub_account_id.to_le_bytes());
    data.extend_from_slice(&[0u8; 32]); // name: all zeros

    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new(user.key(), false),
                AccountMeta::new(user_stats.key(), false),
                AccountMeta::new(state.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new(payer.key(), true),
                AccountMeta::new_readonly(rent.key(), false),
                AccountMeta::new_readonly(system_program.key(), false),
            ],
            data,
        },
        &[
            user.clone(),
            user_stats.clone(),
            state.clone(),
            authority.clone(),
            payer.clone(),
            rent.clone(),
            system_program.clone(),
        ],
    )?;
    Ok(())
}

/// CPI: Drift `deposit`.
/// Transfers USDC from user_token_account into the spot market vault (lending).
/// Accounts (IDL order):
///   0. state               (r)
///   1. user                (mut)
///   2. user_stats          (mut)
///   3. authority           (signer)
///   4. spot_market_vault   (mut — receives USDC)
///   5. user_token_account  (mut — source USDC)
///   6. token_program       (r)
/// Data: discriminator(8) + market_index: u16 LE + amount: u64 LE + reduce_only: bool (1 byte)
pub fn cpi_deposit<'info>(
    drift_program: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    user: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    spot_market_vault: &AccountInfo<'info>,
    user_token_account: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    market_index: u16,
    amount: u64,
) -> Result<()> {
    let mut data = DISC_DEPOSIT.to_vec();
    data.extend_from_slice(&market_index.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(0); // reduce_only = false

    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(state.key(), false),
                AccountMeta::new(user.key(), false),
                AccountMeta::new(user_stats.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new(spot_market_vault.key(), false),
                AccountMeta::new(user_token_account.key(), false),
                AccountMeta::new_readonly(token_program.key(), false),
            ],
            data,
        },
        &[
            state.clone(),
            user.clone(),
            user_stats.clone(),
            authority.clone(),
            spot_market_vault.clone(),
            user_token_account.clone(),
            token_program.clone(),
        ],
    )?;
    Ok(())
}

/// CPI: Drift `withdraw`.
/// Transfers USDC from the spot market vault back to user_token_account.
/// Accounts (IDL order):
///   0. state               (r)
///   1. user                (mut)
///   2. user_stats          (mut)
///   3. authority           (signer)
///   4. spot_market_vault   (mut — source USDC)
///   5. drift_signer        (r — Drift vault authority PDA)
///   6. user_token_account  (mut — receives USDC)
///   7. token_program       (r)
/// Data: discriminator(8) + market_index: u16 LE + amount: u64 LE + reduce_only: bool (1 byte)
pub fn cpi_withdraw<'info>(
    drift_program: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    user: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    spot_market_vault: &AccountInfo<'info>,
    drift_signer: &AccountInfo<'info>,
    user_token_account: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    market_index: u16,
    amount: u64,
) -> Result<()> {
    let mut data = DISC_WITHDRAW.to_vec();
    data.extend_from_slice(&market_index.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(0); // reduce_only = false

    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(state.key(), false),
                AccountMeta::new(user.key(), false),
                AccountMeta::new(user_stats.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new(spot_market_vault.key(), false),
                AccountMeta::new_readonly(drift_signer.key(), false),
                AccountMeta::new(user_token_account.key(), false),
                AccountMeta::new_readonly(token_program.key(), false),
            ],
            data,
        },
        &[
            state.clone(),
            user.clone(),
            user_stats.clone(),
            authority.clone(),
            spot_market_vault.clone(),
            drift_signer.clone(),
            user_token_account.clone(),
            token_program.clone(),
        ],
    )?;
    Ok(())
}
