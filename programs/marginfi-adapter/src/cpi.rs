use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    },
};

// sha256("global:marginfi_account_initialize")[0..8]
pub const DISC_INIT: [u8; 8] = [0x2b, 0x4e, 0x3d, 0xff, 0x94, 0x34, 0xf9, 0x9a];
// sha256("global:lending_account_deposit")[0..8]
pub const DISC_DEPOSIT: [u8; 8] = [0xab, 0x5e, 0xeb, 0x67, 0x52, 0x40, 0xd4, 0x8c];
// sha256("global:lending_account_withdraw")[0..8]
pub const DISC_WITHDRAW: [u8; 8] = [0x24, 0x48, 0x4a, 0x13, 0xd2, 0xd2, 0xc0, 0xc0];

pub const MARGINFI_PROGRAM_ID: Pubkey = pubkey!("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA");
// Group is passed as an account so the adapter works in both mainnet and test environments.
pub const USDC_BANK: Pubkey = pubkey!("2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB");

/// The MarginFi v2 seeds for a MarginfiAccount PDA.
/// marginfi_account = PDA([MARGINFI_ACCOUNT_SEED, group, authority], marginfi_program)
pub const MARGINFI_ACCOUNT_SEED: &[u8] = b"marginfi_account";

/// The MarginFi v2 seeds for a bank's liquidity vault authority PDA.
/// vault_auth = PDA([VAULT_AUTH_SEED, bank], marginfi_program)
pub const VAULT_AUTH_SEED: &[u8] = b"liquidity_vault_auth";

/// I80F48 fixed-point ONE: value 1.0 = 2^48.
pub const ONE_I80F48: u128 = 1u128 << 48;

/// Bank account layout (repr(C), zero_copy, offset includes 8-byte discriminator):
///   [0..8]   discriminator
///   [8..40]  mint: Pubkey
///   [40..41] mint_decimals: u8
///   [41..48] _pad0: [u8; 7]
///   [48..80] group: Pubkey
///   [80..88] _pad1: [u8; 8]
///   [88..104] asset_share_value: WrappedI80F48 (i128 LE)
pub const ASSET_SHARE_VALUE_OFFSET: usize = 88;

/// Read the Bank's asset_share_value (I80F48) as u128.
/// asset_share_value starts at 1.0 (= ONE_I80F48) and increases as interest accrues.
pub fn read_asset_share_value(bank: &AccountInfo) -> Result<u128> {
    let data = bank.try_borrow_data()?;
    require!(
        data.len() >= ASSET_SHARE_VALUE_OFFSET + 16,
        ErrorCode::AccountDidNotDeserialize
    );
    let bytes: [u8; 16] = data[ASSET_SHARE_VALUE_OFFSET..ASSET_SHARE_VALUE_OFFSET + 16]
        .try_into()
        .unwrap();
    let val = i128::from_le_bytes(bytes);
    // asset_share_value is always >= 1.0, so positive
    require!(val > 0, ErrorCode::AccountDidNotDeserialize);
    Ok(val as u128)
}

/// Compute shares from a USDC lamport amount at the current exchange rate.
/// shares = amount * ONE_I80F48 / asset_share_value
pub fn amount_to_shares(amount: u64, asset_share_value: u128) -> Option<u64> {
    let shares = (amount as u128)
        .checked_mul(ONE_I80F48)?
        .checked_div(asset_share_value)?;
    u64::try_from(shares).ok()
}

/// Compute USDC lamport value of shares at the current exchange rate.
/// value = shares * asset_share_value / ONE_I80F48
pub fn shares_to_amount(shares: u64, asset_share_value: u128) -> Option<u64> {
    let value = (shares as u128)
        .checked_mul(asset_share_value)?
        .checked_div(ONE_I80F48)?;
    u64::try_from(value).ok()
}

/// CPI: MarginFi `marginfi_account_initialize`.
/// Accounts order (from MarginFi IDL):
///   0. marginfi_group  (r)
///   1. marginfi_account (mut)
///   2. authority        (signer — our adapter PDA)
///   3. fee_payer        (mut, signer — user)
///   4. system_program   (r)
pub fn cpi_initialize_account<'info>(
    marginfi_program: &AccountInfo<'info>,
    marginfi_group: &AccountInfo<'info>,
    marginfi_account: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    fee_payer: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    invoke_signed(
        &Instruction {
            program_id: marginfi_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(marginfi_group.key(), false),
                AccountMeta::new(marginfi_account.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new(fee_payer.key(), true),
                AccountMeta::new_readonly(system_program.key(), false),
            ],
            data: DISC_INIT.to_vec(),
        },
        &[
            marginfi_group.clone(),
            marginfi_account.clone(),
            authority.clone(),
            fee_payer.clone(),
            system_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}

/// CPI: MarginFi `lending_account_deposit`.
/// Accounts order (from MarginFi IDL):
///   0. marginfi_group        (r)
///   1. marginfi_account      (mut)
///   2. signer / authority    (signer — our adapter PDA)
///   3. bank                  (mut)
///   4. signer_token_account  (mut — user's USDC ATA, source)
///   5. bank_liquidity_vault  (mut — MarginFi USDC vault, destination)
///   6. token_program         (r)
pub fn cpi_deposit<'info>(
    marginfi_program: &AccountInfo<'info>,
    marginfi_group: &AccountInfo<'info>,
    marginfi_account: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    bank: &AccountInfo<'info>,
    signer_token_account: &AccountInfo<'info>,
    bank_liquidity_vault: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_DEPOSIT.to_vec();
    data.extend_from_slice(&amount.to_le_bytes());

    invoke_signed(
        &Instruction {
            program_id: marginfi_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(marginfi_group.key(), false),
                AccountMeta::new(marginfi_account.key(), false),
                AccountMeta::new_readonly(authority.key(), true),
                AccountMeta::new(bank.key(), false),
                AccountMeta::new(signer_token_account.key(), false),
                AccountMeta::new(bank_liquidity_vault.key(), false),
                AccountMeta::new_readonly(token_program.key(), false),
            ],
            data,
        },
        &[
            marginfi_group.clone(),
            marginfi_account.clone(),
            authority.clone(),
            bank.clone(),
            signer_token_account.clone(),
            bank_liquidity_vault.clone(),
            token_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}

/// CPI: MarginFi `lending_account_withdraw`.
/// Accounts order (from MarginFi IDL):
///   0. marginfi_group               (r)
///   1. marginfi_account             (mut)
///   2. signer / authority           (signer — our adapter PDA)
///   3. bank                         (mut)
///   4. destination_token_account    (mut — user's USDC ATA, destination)
///   5. bank_liquidity_vault_auth    (r — MarginFi vault authority PDA)
///   6. bank_liquidity_vault         (mut — MarginFi USDC vault, source)
///   7. token_program                (r)
/// Remaining accounts: oracle accounts required by MarginFi for health check.
pub fn cpi_withdraw<'info>(
    marginfi_program: &AccountInfo<'info>,
    marginfi_group: &AccountInfo<'info>,
    marginfi_account: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    bank: &AccountInfo<'info>,
    destination_token_account: &AccountInfo<'info>,
    bank_liquidity_vault_authority: &AccountInfo<'info>,
    bank_liquidity_vault: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    oracle_accounts: &[AccountInfo<'info>],
    amount: u64,
    withdraw_all: bool,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_WITHDRAW.to_vec();
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(withdraw_all as u8);

    let mut account_metas = vec![
        AccountMeta::new_readonly(marginfi_group.key(), false),
        AccountMeta::new(marginfi_account.key(), false),
        AccountMeta::new_readonly(authority.key(), true),
        AccountMeta::new(bank.key(), false),
        AccountMeta::new(destination_token_account.key(), false),
        AccountMeta::new_readonly(bank_liquidity_vault_authority.key(), false),
        AccountMeta::new(bank_liquidity_vault.key(), false),
        AccountMeta::new_readonly(token_program.key(), false),
    ];
    for oracle in oracle_accounts {
        account_metas.push(AccountMeta::new_readonly(oracle.key(), false));
    }

    let mut account_infos = vec![
        marginfi_group.clone(),
        marginfi_account.clone(),
        authority.clone(),
        bank.clone(),
        destination_token_account.clone(),
        bank_liquidity_vault_authority.clone(),
        bank_liquidity_vault.clone(),
        token_program.clone(),
    ];
    account_infos.extend_from_slice(oracle_accounts);

    invoke_signed(
        &Instruction {
            program_id: marginfi_program.key(),
            accounts: account_metas,
            data,
        },
        &account_infos,
        signer_seeds,
    )?;
    Ok(())
}
