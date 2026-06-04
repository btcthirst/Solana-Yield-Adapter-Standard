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

/// Mainnet USDC spot market (market_index = 0)
pub const USDC_SPOT_MARKET: Pubkey = pubkey!("6gMq3mRCKf8aP3ttTyYhuijVZ2LGi14oDsBbkgubfLB3");
/// Insurance fund vault for the USDC spot market
pub const USDC_IF_VAULT: Pubkey    = pubkey!("2CqkQvYxp9Mq4PqLvAQ1eryYxebUh4Liyn5YMDtXsYci");

pub const MARKET_INDEX: u16 = 0;

// ── Instruction discriminators (sha256("global:{name}")[0..8]) ───────────────

/// sha256("global:initialize_user_stats")[0..8]
pub const DISC_INIT_USER_STATS: [u8; 8] = [0xfe, 0xf3, 0x48, 0x62, 0xfb, 0x82, 0xa8, 0xd5];
/// sha256("global:initialize_insurance_fund_stake")[0..8]
pub const DISC_INIT_IF_STAKE: [u8; 8]   = [0xbb, 0xb3, 0xf3, 0x46, 0xf8, 0x5a, 0x5c, 0x93];
/// sha256("global:add_insurance_fund_stake")[0..8]
pub const DISC_ADD_IF_STAKE: [u8; 8]    = [0xfb, 0x90, 0x73, 0x0b, 0xde, 0x2f, 0x3e, 0xec];
/// sha256("global:request_remove_insurance_fund_stake")[0..8]
pub const DISC_REQUEST_REMOVE: [u8; 8]  = [0x8e, 0x46, 0xcc, 0x5c, 0x49, 0x6a, 0xb4, 0x34];
/// sha256("global:remove_insurance_fund_stake")[0..8]
pub const DISC_REMOVE_IF_STAKE: [u8; 8] = [0x80, 0xa6, 0x8e, 0x09, 0xfe, 0xbb, 0x8f, 0xae];

// ── Byte-offset readers ──────────────────────────────────────────────────────

/// InsuranceFundStake layout (zero_copy repr(C), including 8-byte discriminator):
///   [0..8]   discriminator
///   [8..40]  authority: Pubkey
///   [40..56] if_shares: u128
///   [56..72] last_withdraw_request_shares: u128
///   [72..88] if_base: u128
///   [88..96] last_valid_ts: i64
///   [96..104] last_withdraw_request_ts: i64
const IF_SHARES_OFFSET: usize = 40;
const IF_SHARES_END: usize = 56;                   // 40 + 16
const LAST_WITHDRAW_REQUEST_TS_OFFSET: usize = 96;
const LAST_WITHDRAW_REQUEST_TS_END: usize = 104;   // 96 + 8
const TOTAL_SHARES_OFFSET: usize = 336;
const TOTAL_SHARES_END: usize = 352;               // 336 + 16
const UNSTAKING_PERIOD_OFFSET: usize = 384;
const UNSTAKING_PERIOD_END: usize = 392;           // 384 + 8
const TOKEN_ACCOUNT_AMOUNT_OFFSET: usize = 64;
const TOKEN_ACCOUNT_AMOUNT_END: usize = 72;        // 64 + 8

pub fn read_if_shares(stake: &AccountInfo) -> Result<u128> {
    let data = stake.try_borrow_data()?;
    require!(data.len() >= IF_SHARES_END, ErrorCode::AccountDidNotDeserialize);
    let bytes: [u8; 16] = data[IF_SHARES_OFFSET..IF_SHARES_END]
        .try_into()
        .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))?;
    Ok(u128::from_le_bytes(bytes))
}

pub fn read_last_withdraw_request_ts(stake: &AccountInfo) -> Result<i64> {
    let data = stake.try_borrow_data()?;
    require!(data.len() >= LAST_WITHDRAW_REQUEST_TS_END, ErrorCode::AccountDidNotDeserialize);
    let bytes: [u8; 8] = data[LAST_WITHDRAW_REQUEST_TS_OFFSET..LAST_WITHDRAW_REQUEST_TS_END]
        .try_into()
        .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))?;
    Ok(i64::from_le_bytes(bytes))
}

pub fn read_total_shares(spot_market: &AccountInfo) -> Result<u128> {
    let data = spot_market.try_borrow_data()?;
    require!(data.len() >= TOTAL_SHARES_END, ErrorCode::AccountDidNotDeserialize);
    let bytes: [u8; 16] = data[TOTAL_SHARES_OFFSET..TOTAL_SHARES_END]
        .try_into()
        .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))?;
    Ok(u128::from_le_bytes(bytes))
}

pub fn read_unstaking_period(spot_market: &AccountInfo) -> Result<i64> {
    let data = spot_market.try_borrow_data()?;
    require!(data.len() >= UNSTAKING_PERIOD_END, ErrorCode::AccountDidNotDeserialize);
    let bytes: [u8; 8] = data[UNSTAKING_PERIOD_OFFSET..UNSTAKING_PERIOD_END]
        .try_into()
        .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))?;
    Ok(i64::from_le_bytes(bytes))
}

pub fn read_token_account_amount(vault: &AccountInfo) -> Result<u64> {
    let data = vault.try_borrow_data()?;
    require!(data.len() >= TOKEN_ACCOUNT_AMOUNT_END, ErrorCode::AccountDidNotDeserialize);
    let bytes: [u8; 8] = data[TOKEN_ACCOUNT_AMOUNT_OFFSET..TOKEN_ACCOUNT_AMOUNT_END]
        .try_into()
        .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))?;
    Ok(u64::from_le_bytes(bytes))
}

// ── CPI functions ────────────────────────────────────────────────────────────
//
// All Drift CPIs use `invoke` (not invoke_signed):
//   - initialize_position: owner is Signer<'info> in the outer context, so their
//     signature is present in the transaction and propagates naturally.
//   - deposit / withdraw: owner is passed from Dispatcher via `invoke`, so
//     their is_signer flag is already set true on the AccountInfo.

/// CPI: Drift `initialize_user_stats`.
/// Accounts (IDL order):
///   0. user_stats  (mut, init PDA — seeds: [b"user_stats", authority])
///   1. state       (mut)
///   2. authority   (signer)
///   3. payer       (mut, signer — same as authority)
///   4. rent        (sysvar)
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

/// CPI: Drift `initialize_insurance_fund_stake`.
/// Accounts (IDL order):
///   0. spot_market          (r)
///   1. insurance_fund_stake (mut, init PDA — seeds: [b"insurance_fund_stake", authority, market_index_le])
///   2. user_stats           (mut)
///   3. authority            (signer)
///   4. payer                (mut, signer — same as authority)
///   5. state                (r)
///   6. rent                 (sysvar)
///   7. system_program
/// Data: discriminator(8) + market_index: u16 LE
pub fn cpi_initialize_insurance_fund_stake<'info>(
    drift_program: &AccountInfo<'info>,
    spot_market: &AccountInfo<'info>,
    insurance_fund_stake: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    rent: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    market_index: u16,
) -> Result<()> {
    let mut data = DISC_INIT_IF_STAKE.to_vec();
    data.extend_from_slice(&market_index.to_le_bytes());

    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(spot_market.key(), false),
                AccountMeta::new(insurance_fund_stake.key(), false),
                AccountMeta::new(user_stats.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new(payer.key(), true),
                AccountMeta::new_readonly(state.key(), false),
                AccountMeta::new_readonly(rent.key(), false),
                AccountMeta::new_readonly(system_program.key(), false),
            ],
            data,
        },
        &[
            spot_market.clone(),
            insurance_fund_stake.clone(),
            user_stats.clone(),
            authority.clone(),
            payer.clone(),
            state.clone(),
            rent.clone(),
            system_program.clone(),
        ],
    )?;
    Ok(())
}

/// CPI: Drift `add_insurance_fund_stake`.
/// Accounts (IDL order):
///   0. state                (r)
///   1. spot_market          (mut)
///   2. insurance_fund_vault (mut — IF vault, destination of staked USDC)
///   3. insurance_fund_stake (mut)
///   4. user_stats           (mut)
///   5. authority            (signer — user, propagated from Dispatcher)
///   6. spot_market_vault    (mut — spot market collateral vault, for revenue settle)
///   7. user_token_account   (mut — user's USDC ATA, source)
///   8. token_program        (r)
/// Data: discriminator(8) + market_index: u16 LE + amount: u64 LE
pub fn cpi_add_insurance_fund_stake<'info>(
    drift_program: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    spot_market: &AccountInfo<'info>,
    insurance_fund_vault: &AccountInfo<'info>,
    insurance_fund_stake: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    spot_market_vault: &AccountInfo<'info>,
    user_token_account: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    market_index: u16,
    amount: u64,
) -> Result<()> {
    let mut data = DISC_ADD_IF_STAKE.to_vec();
    data.extend_from_slice(&market_index.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());

    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(state.key(), false),
                AccountMeta::new(spot_market.key(), false),
                AccountMeta::new(insurance_fund_vault.key(), false),
                AccountMeta::new(insurance_fund_stake.key(), false),
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
            spot_market.clone(),
            insurance_fund_vault.clone(),
            insurance_fund_stake.clone(),
            user_stats.clone(),
            authority.clone(),
            spot_market_vault.clone(),
            user_token_account.clone(),
            token_program.clone(),
        ],
    )?;
    Ok(())
}

/// CPI: Drift `request_remove_insurance_fund_stake`.
/// Initiates the cooldown countdown. Drift records `lastWithdrawRequestShares = if_shares`
/// and `lastWithdrawRequestTs = now` in the InsuranceFundStake account.
/// Accounts (IDL order):
///   0. state                (r)
///   1. spot_market          (mut)
///   2. insurance_fund_stake (mut)
///   3. user_stats           (mut)
///   4. authority            (signer)
/// Data: discriminator(8) + market_index: u16 LE
pub fn cpi_request_remove_insurance_fund_stake<'info>(
    drift_program: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    spot_market: &AccountInfo<'info>,
    insurance_fund_stake: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    market_index: u16,
) -> Result<()> {
    let mut data = DISC_REQUEST_REMOVE.to_vec();
    data.extend_from_slice(&market_index.to_le_bytes());

    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(state.key(), false),
                AccountMeta::new(spot_market.key(), false),
                AccountMeta::new(insurance_fund_stake.key(), false),
                AccountMeta::new(user_stats.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
            ],
            data,
        },
        &[
            state.clone(),
            spot_market.clone(),
            insurance_fund_stake.clone(),
            user_stats.clone(),
            authority.clone(),
        ],
    )?;
    Ok(())
}

/// CPI: Drift `remove_insurance_fund_stake`.
/// Executes the withdrawal after the cooldown has elapsed.
/// Drift transfers USDC from insurance_fund_vault → user_token_account.
/// Accounts (IDL order):
///   0. state                (r)
///   1. spot_market          (mut)
///   2. insurance_fund_vault (mut — source of withdrawn USDC)
///   3. insurance_fund_stake (mut)
///   4. user_stats           (mut)
///   5. authority            (signer)
///   6. drift_signer         (r — Drift's vault authority PDA)
///   7. user_token_account   (mut — destination, user's USDC ATA)
///   8. token_program        (r)
/// Data: discriminator(8) + market_index: u16 LE
pub fn cpi_remove_insurance_fund_stake<'info>(
    drift_program: &AccountInfo<'info>,
    state: &AccountInfo<'info>,
    spot_market: &AccountInfo<'info>,
    insurance_fund_vault: &AccountInfo<'info>,
    insurance_fund_stake: &AccountInfo<'info>,
    user_stats: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    drift_signer: &AccountInfo<'info>,
    user_token_account: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    market_index: u16,
) -> Result<()> {
    let mut data = DISC_REMOVE_IF_STAKE.to_vec();
    data.extend_from_slice(&market_index.to_le_bytes());

    invoke(
        &Instruction {
            program_id: drift_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(state.key(), false),
                AccountMeta::new(spot_market.key(), false),
                AccountMeta::new(insurance_fund_vault.key(), false),
                AccountMeta::new(insurance_fund_stake.key(), false),
                AccountMeta::new(user_stats.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new_readonly(drift_signer.key(), false),
                AccountMeta::new(user_token_account.key(), false),
                AccountMeta::new_readonly(token_program.key(), false),
            ],
            data,
        },
        &[
            state.clone(),
            spot_market.clone(),
            insurance_fund_vault.clone(),
            insurance_fund_stake.clone(),
            user_stats.clone(),
            authority.clone(),
            drift_signer.clone(),
            user_token_account.clone(),
            token_program.clone(),
        ],
    )?;
    Ok(())
}
