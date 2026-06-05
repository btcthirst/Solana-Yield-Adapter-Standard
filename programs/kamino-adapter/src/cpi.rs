use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::{invoke, invoke_signed},
    },
};

/// Kamino Lending program ID (mainnet).
pub const KLEND_PROGRAM_ID: Pubkey = pubkey!("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD");

/// Rent sysvar.
pub const RENT_SYSVAR_ID: Pubkey = pubkey!("SysvarRent111111111111111111111111111111111");
/// Instructions sysvar.
pub const INSTRUCTIONS_SYSVAR_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

/// Main Kamino lending market.
pub const MAIN_LENDING_MARKET: Pubkey = pubkey!("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF");

/// USDC mint (mainnet).
pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

// --- Lending market authority seeds (Kamino uses b"lma")
pub const LENDING_MARKET_AUTH_SEED: &[u8] = b"lma";

// --- User metadata seeds
pub const USER_METADATA_SEED: &[u8] = b"user_meta";

// --- Instruction discriminators (sha256("global:{name}")[0..8])

/// sha256("global:init_user_metadata")[0..8]
pub const DISC_INIT_USER_METADATA: [u8; 8] = [0x75, 0xa9, 0xb0, 0x45, 0xc5, 0x17, 0x0f, 0xa2];
/// sha256("global:init_obligation")[0..8]
pub const DISC_INIT_OBLIGATION: [u8; 8] = [0xfb, 0x0a, 0xe7, 0x4c, 0x1b, 0x0b, 0x9f, 0x60];
/// sha256("global:refresh_reserve")[0..8]
pub const DISC_REFRESH_RESERVE: [u8; 8] = [0x02, 0xda, 0x8a, 0xeb, 0x4f, 0xc9, 0x19, 0x66];
/// sha256("global:refresh_obligation")[0..8]
pub const DISC_REFRESH_OBLIGATION: [u8; 8] = [0x21, 0x84, 0x93, 0xe4, 0x97, 0xc0, 0x48, 0x59];
/// sha256("global:deposit_reserve_liquidity_and_obligation_collateral_v2")[0..8]
/// V2 variant — required when the reserve has an active farm (carries farm accounts).
pub const DISC_DEPOSIT_V2: [u8; 8] = [0xd8, 0xe0, 0xbf, 0x1b, 0xcc, 0x97, 0x66, 0xaf];
/// sha256("global:withdraw_obligation_collateral_and_redeem_reserve_collateral_v2")[0..8]
pub const DISC_WITHDRAW_V2: [u8; 8] = [0xeb, 0x34, 0x77, 0x98, 0x95, 0xc5, 0x14, 0x07];
/// sha256("global:init_obligation_farms_for_reserve")[0..8]
pub const DISC_INIT_FARMS: [u8; 8] = [0x88, 0x3f, 0x0f, 0xba, 0xd3, 0x98, 0xa8, 0xa4];

/// Kamino Farms program ID.
pub const FARMS_PROGRAM_ID: Pubkey = pubkey!("FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr");

// --- Reserve data offsets (absolute, including 8-byte Anchor discriminator prefix)
//
// Reserve layout:
//   [0..8]     discriminator
//   [8..16]    version: u64
//   [16..32]   lastUpdate: { slot: u64(8), stale: u8(1), priceStatus: u8(1), placeholder: [u8;6](6) }
//   [32..64]   lendingMarket: Pubkey
//   [64..96]   farmCollateral: Pubkey
//   [96..128]  farmDebt: Pubkey
//   [128..]    liquidity: ReserveLiquidity
//     [128..160]  mintPubkey: Pubkey
//     [160..192]  supplyVault: Pubkey
//     [192..224]  feeVault: Pubkey
//     [224..232]  availableAmount: u64         ← AVAILABLE_AMOUNT_OFFSET
//     [232..248]  borrowedAmountSf: u128        ← BORROWED_AMOUNT_SF_OFFSET
//     ... (1232 bytes total for ReserveLiquidity)
//   [1360..2560] reserveLiquidityPadding: [u64;150]
//   [2560..]   collateral: ReserveCollateral
//     [2560..2592] mintPubkey: Pubkey
//     [2592..2600] mintTotalSupply: u64         ← CTOKEN_SUPPLY_OFFSET
//     [2600..2632] supplyVault: Pubkey

pub const AVAILABLE_AMOUNT_OFFSET: usize = 224;
pub const BORROWED_AMOUNT_SF_OFFSET: usize = 232;
pub const CTOKEN_SUPPLY_OFFSET: usize = 2592;

/// Kamino's _sf (Scaled Fraction) denominator: 2^60.
pub const FRACTION_ONE_SF: u128 = 1u128 << 60;

// ── Reserve helpers ────────────────────────────────────────────────────────────

/// Read total USDC liquidity (available + borrowed, in lamports) and total
/// ctoken supply from raw Reserve account data.
///
/// Returns (total_liquidity, ctoken_supply).
pub fn read_reserve_exchange_data(
    reserve_data: &[u8],
) -> Result<(u64, u64)> {
    require!(
        reserve_data.len() > CTOKEN_SUPPLY_OFFSET + 8,
        crate::error::AdapterError::ReserveDataTooShort
    );

    let available = u64::from_le_bytes(
        reserve_data[AVAILABLE_AMOUNT_OFFSET..AVAILABLE_AMOUNT_OFFSET + 8]
            .try_into()
            .unwrap(),
    );

    let borrowed_sf = u128::from_le_bytes(
        reserve_data[BORROWED_AMOUNT_SF_OFFSET..BORROWED_AMOUNT_SF_OFFSET + 16]
            .try_into()
            .unwrap(),
    );
    let borrowed = (borrowed_sf / FRACTION_ONE_SF) as u64;

    let total_liquidity = available
        .checked_add(borrowed)
        .ok_or(error!(crate::error::AdapterError::Overflow))?;

    let ctoken_supply = u64::from_le_bytes(
        reserve_data[CTOKEN_SUPPLY_OFFSET..CTOKEN_SUPPLY_OFFSET + 8]
            .try_into()
            .unwrap(),
    );

    Ok((total_liquidity, ctoken_supply))
}

/// Compute ctokens received for a given USDC deposit amount.
/// ctokens = usdc_amount * ctoken_supply / total_liquidity
pub fn amount_to_ctokens(
    usdc_amount: u64,
    total_liquidity: u64,
    ctoken_supply: u64,
) -> Option<u64> {
    if total_liquidity == 0 || ctoken_supply == 0 {
        // Reserve is empty: 1:1 exchange rate.
        return Some(usdc_amount);
    }
    let ctokens = (usdc_amount as u128)
        .checked_mul(ctoken_supply as u128)?
        .checked_div(total_liquidity as u128)?;
    u64::try_from(ctokens).ok()
}

/// Compute USDC value for a given ctoken count.
/// value = ctokens * total_liquidity / ctoken_supply
pub fn ctokens_to_amount(
    ctokens: u64,
    total_liquidity: u64,
    ctoken_supply: u64,
) -> Option<u64> {
    if ctoken_supply == 0 {
        return Some(ctokens);
    }
    let value = (ctokens as u128)
        .checked_mul(total_liquidity as u128)?
        .checked_div(ctoken_supply as u128)?;
    u64::try_from(value).ok()
}

// ── SPL token helpers ───────────────────────────────────────────────────────────

/// Read an SPL token account's `amount` (offset 64..72).
pub fn read_token_amount(token_account: &AccountInfo) -> Result<u64> {
    let data = token_account.try_borrow_data()?;
    require!(data.len() >= 72, crate::error::AdapterError::ProtocolError);
    Ok(u64::from_le_bytes(data[64..72].try_into().unwrap()))
}

/// SPL Token `transfer` (instruction 3) signed by a PDA authority.
pub fn cpi_token_transfer<'info>(
    token_program: &AccountInfo<'info>,
    src: &AccountInfo<'info>,
    dst: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = vec![3u8]; // SPL Token: Transfer
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

// ── CPI helpers ───────────────────────────────────────────────────────────────

/// CPI: Kamino `init_user_metadata`.
///
/// Accounts:
///   0. owner (signer)
///   1. feePayer (mut, signer)
///   2. userMetadata (mut)
///   3. referrerUserMetadata (optional — pass KLEND_PROGRAM_ID)
///   4. rent
///   5. systemProgram
///
/// Args: user_lookup_table (Pubkey) — pass Pubkey::default() if unused.
pub fn cpi_init_user_metadata<'info>(
    klend_program: &AccountInfo<'info>,
    owner: &AccountInfo<'info>,         // kamino_authority PDA
    fee_payer: &AccountInfo<'info>,
    user_metadata: &AccountInfo<'info>,
    rent: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_INIT_USER_METADATA.to_vec();
    // user_lookup_table: Pubkey (32 zero bytes = default/SystemProgram)
    data.extend_from_slice(&[0u8; 32]);

    invoke_signed(
        &Instruction {
            program_id: klend_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(owner.key(), true),      // owner (signer)
                AccountMeta::new(fee_payer.key(), true),            // feePayer (mut, signer)
                AccountMeta::new(user_metadata.key(), false),       // userMetadata (mut)
                AccountMeta::new_readonly(KLEND_PROGRAM_ID, false), // referrerUserMetadata (optional)
                AccountMeta::new_readonly(RENT_SYSVAR_ID, false), // rent
                AccountMeta::new_readonly(system_program.key(), false), // systemProgram
            ],
            data,
        },
        &[
            owner.clone(),
            fee_payer.clone(),
            user_metadata.clone(),
            rent.clone(),
            system_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}

/// CPI: Kamino `init_obligation`.
///
/// Accounts (order from IDL):
///   0. obligationOwner (signer)
///   1. feePayer (mut, signer)
///   2. obligation (mut)
///   3. lendingMarket
///   4. seed1Account
///   5. seed2Account
///   6. ownerUserMetadata
///   7. rent
///   8. systemProgram
///
/// Args: tag=0, id=0 (VanillaObligation).
pub fn cpi_init_obligation<'info>(
    klend_program: &AccountInfo<'info>,
    obligation_owner: &AccountInfo<'info>, // kamino_authority PDA
    fee_payer: &AccountInfo<'info>,
    obligation: &AccountInfo<'info>,
    lending_market: &AccountInfo<'info>,
    seed1: &AccountInfo<'info>,            // SystemProgram
    seed2: &AccountInfo<'info>,            // SystemProgram
    owner_user_metadata: &AccountInfo<'info>,
    rent: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_INIT_OBLIGATION.to_vec();
    // InitObligationArgs { tag: u8, id: u8 }
    data.push(0u8); // tag = 0 (VanillaObligation)
    data.push(0u8); // id = 0

    invoke_signed(
        &Instruction {
            program_id: klend_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(obligation_owner.key(), true), // obligationOwner
                AccountMeta::new(fee_payer.key(), true),                  // feePayer
                AccountMeta::new(obligation.key(), false),                // obligation
                AccountMeta::new_readonly(lending_market.key(), false),   // lendingMarket
                AccountMeta::new_readonly(seed1.key(), false),            // seed1
                AccountMeta::new_readonly(seed2.key(), false),            // seed2
                AccountMeta::new_readonly(owner_user_metadata.key(), false), // ownerUserMetadata
                AccountMeta::new_readonly(RENT_SYSVAR_ID, false),       // rent
                AccountMeta::new_readonly(system_program.key(), false),   // systemProgram
            ],
            data,
        },
        &[
            obligation_owner.clone(),
            fee_payer.clone(),
            obligation.clone(),
            lending_market.clone(),
            seed1.clone(),
            seed2.clone(),
            owner_user_metadata.clone(),
            rent.clone(),
            system_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}

/// CPI: Kamino `refresh_reserve`.
///
/// Accounts:
///   0. reserve (mut)
///   1. lendingMarket
///   2. pythOracle (optional)
///   3. switchboardPriceOracle (optional)
///   4. switchboardTwapOracle (optional)
///   5. scopePrices (optional)
///
/// Pass an account with key == KLEND_PROGRAM_ID for any oracle you don't have.
/// oracle_accounts: owned array of exactly 4 AccountInfos.
pub fn cpi_refresh_reserve<'info>(
    klend_program: &AccountInfo<'info>,
    reserve: &AccountInfo<'info>,
    lending_market: &AccountInfo<'info>,
    oracle_accounts: [AccountInfo<'info>; 4],
) -> Result<()> {
    let data = DISC_REFRESH_RESERVE.to_vec();

    let accounts = vec![
        AccountMeta::new(reserve.key(), false),
        AccountMeta::new_readonly(lending_market.key(), false),
        AccountMeta::new_readonly(oracle_accounts[0].key(), false),
        AccountMeta::new_readonly(oracle_accounts[1].key(), false),
        AccountMeta::new_readonly(oracle_accounts[2].key(), false),
        AccountMeta::new_readonly(oracle_accounts[3].key(), false),
    ];

    let account_infos = vec![
        reserve.clone(),
        lending_market.clone(),
        oracle_accounts[0].clone(),
        oracle_accounts[1].clone(),
        oracle_accounts[2].clone(),
        oracle_accounts[3].clone(),
    ];

    invoke_signed(
        &Instruction {
            program_id: klend_program.key(),
            accounts,
            data,
        },
        &account_infos,
        &[],
    )?;
    Ok(())
}

/// CPI: Kamino `refresh_obligation`.
///
/// Accounts:
///   0. lendingMarket
///   1. obligation (mut)
///   remaining: [reserve] — all reserves the obligation has deposits in.
///
/// For our single-reserve USDC adapter, remaining = [usdc_reserve].
pub fn cpi_refresh_obligation<'info>(
    klend_program: &AccountInfo<'info>,
    lending_market: &AccountInfo<'info>,
    obligation: &AccountInfo<'info>,
    reserves: &[AccountInfo<'info>],
) -> Result<()> {
    let data = DISC_REFRESH_OBLIGATION.to_vec();

    let mut accounts = vec![
        AccountMeta::new_readonly(lending_market.key(), false),
        AccountMeta::new(obligation.key(), false),
    ];
    for r in reserves {
        accounts.push(AccountMeta::new(r.key(), false));
    }

    let mut account_infos = vec![lending_market.clone(), obligation.clone()];
    account_infos.extend_from_slice(reserves);

    invoke_signed(
        &Instruction {
            program_id: klend_program.key(),
            accounts,
            data,
        },
        &account_infos,
        &[],
    )?;
    Ok(())
}

/// CPI: Kamino `init_obligation_farms_for_reserve` (mode 0 = collateral farm).
///
/// Creates the obligation's farm-user-state, required before a V2 deposit into a
/// reserve that has an active farm. `owner` (obligation owner) is NOT a signer
/// here — the instruction is permissionless; `payer` (the real wallet) signs.
///
/// Accounts (from IDL):
///   0.  payer (mut, signer)
///   1.  owner
///   2.  obligation (mut)
///   3.  lendingMarketAuthority
///   4.  reserve (mut)
///   5.  reserveFarmState (mut)
///   6.  obligationFarm (mut)
///   7.  lendingMarket
///   8.  farmsProgram
///   9.  rent
///   10. systemProgram
///
/// Args: mode (u8).
#[allow(clippy::too_many_arguments)]
pub fn cpi_init_obligation_farms_for_reserve<'info>(
    klend_program: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    owner: &AccountInfo<'info>,
    obligation: &AccountInfo<'info>,
    lending_market_authority: &AccountInfo<'info>,
    reserve: &AccountInfo<'info>,
    reserve_farm_state: &AccountInfo<'info>,
    obligation_farm: &AccountInfo<'info>,
    lending_market: &AccountInfo<'info>,
    farms_program: &AccountInfo<'info>,
    rent: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    mode: u8,
) -> Result<()> {
    let mut data = DISC_INIT_FARMS.to_vec();
    data.push(mode);

    invoke(
        &Instruction {
            program_id: klend_program.key(),
            accounts: vec![
                AccountMeta::new(payer.key(), true),                              // payer
                AccountMeta::new_readonly(owner.key(), false),                    // owner
                AccountMeta::new(obligation.key(), false),                        // obligation
                AccountMeta::new_readonly(lending_market_authority.key(), false), // lendingMarketAuthority
                AccountMeta::new(reserve.key(), false),                           // reserve
                AccountMeta::new(reserve_farm_state.key(), false),                // reserveFarmState
                AccountMeta::new(obligation_farm.key(), false),                   // obligationFarm
                AccountMeta::new_readonly(lending_market.key(), false),           // lendingMarket
                AccountMeta::new_readonly(farms_program.key(), false),            // farmsProgram
                AccountMeta::new_readonly(RENT_SYSVAR_ID, false),                 // rent
                AccountMeta::new_readonly(system_program.key(), false),           // systemProgram
            ],
            data,
        },
        &[
            payer.clone(),
            owner.clone(),
            obligation.clone(),
            lending_market_authority.clone(),
            reserve.clone(),
            reserve_farm_state.clone(),
            obligation_farm.clone(),
            lending_market.clone(),
            farms_program.clone(),
            rent.clone(),
            system_program.clone(),
        ],
    )?;
    Ok(())
}

/// CPI: Kamino `deposit_reserve_liquidity_and_obligation_collateral_v2`.
///
/// V2 carries the farm accounts (the USDC main-market reserve has an active
/// farm, so the V1 instruction is rejected with CpiDisabled when CPI'd).
///
/// Accounts (deposit group, then farms group):
///   0.  owner (signer, mut) — kamino_authority PDA
///   1.  obligation (mut)
///   2.  lendingMarket
///   3.  lendingMarketAuthority
///   4.  reserve (mut)
///   5.  reserveLiquidityMint
///   6.  reserveLiquiditySupply (mut)
///   7.  reserveCollateralMint (mut)
///   8.  reserveDestinationDepositCollateral (mut)
///   9.  userSourceLiquidity (mut)
///   10. placeholderUserDestinationCollateral (readonly)
///   11. collateralTokenProgram
///   12. liquidityTokenProgram
///   13. instructionSysvarAccount
///   14. obligationFarmUserState (mut)
///   15. reserveFarmState (mut)
///   16. farmsProgram
///
/// Args: liquidity_amount (u64).
#[allow(clippy::too_many_arguments)]
pub fn cpi_deposit<'info>(
    klend_program: &AccountInfo<'info>,
    owner: &AccountInfo<'info>,
    obligation: &AccountInfo<'info>,
    lending_market: &AccountInfo<'info>,
    lending_market_authority: &AccountInfo<'info>,
    reserve: &AccountInfo<'info>,
    reserve_liquidity_mint: &AccountInfo<'info>,
    reserve_liquidity_supply: &AccountInfo<'info>,
    reserve_collateral_mint: &AccountInfo<'info>,
    reserve_destination_deposit_collateral: &AccountInfo<'info>,
    user_source_liquidity: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    instruction_sysvar: &AccountInfo<'info>,
    obligation_farm_user_state: &AccountInfo<'info>,
    reserve_farm_state: &AccountInfo<'info>,
    farms_program: &AccountInfo<'info>,
    liquidity_amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_DEPOSIT_V2.to_vec();
    data.extend_from_slice(&liquidity_amount.to_le_bytes());

    invoke_signed(
        &Instruction {
            program_id: klend_program.key(),
            accounts: vec![
                AccountMeta::new(owner.key(), true),                                     // owner
                AccountMeta::new(obligation.key(), false),                               // obligation
                AccountMeta::new_readonly(lending_market.key(), false),                  // lendingMarket
                AccountMeta::new_readonly(lending_market_authority.key(), false),        // lendingMarketAuthority
                AccountMeta::new(reserve.key(), false),                                  // reserve
                AccountMeta::new_readonly(reserve_liquidity_mint.key(), false),          // reserveLiquidityMint
                AccountMeta::new(reserve_liquidity_supply.key(), false),                 // reserveLiquiditySupply
                AccountMeta::new(reserve_collateral_mint.key(), false),                  // reserveCollateralMint
                AccountMeta::new(reserve_destination_deposit_collateral.key(), false),   // reserveDestinationDepositCollateral
                AccountMeta::new(user_source_liquidity.key(), false),                    // userSourceLiquidity
                AccountMeta::new_readonly(KLEND_PROGRAM_ID, false),                      // placeholderUserDestinationCollateral
                AccountMeta::new_readonly(token_program.key(), false),                   // collateralTokenProgram
                AccountMeta::new_readonly(token_program.key(), false),                   // liquidityTokenProgram
                AccountMeta::new_readonly(instruction_sysvar.key(), false),              // instructionSysvarAccount
                AccountMeta::new(obligation_farm_user_state.key(), false),               // obligationFarmUserState
                AccountMeta::new(reserve_farm_state.key(), false),                       // reserveFarmState
                AccountMeta::new_readonly(farms_program.key(), false),                   // farmsProgram
            ],
            data,
        },
        &[
            owner.clone(),
            obligation.clone(),
            lending_market.clone(),
            lending_market_authority.clone(),
            reserve.clone(),
            reserve_liquidity_mint.clone(),
            reserve_liquidity_supply.clone(),
            reserve_collateral_mint.clone(),
            reserve_destination_deposit_collateral.clone(),
            user_source_liquidity.clone(),
            token_program.clone(),
            token_program.clone(),
            instruction_sysvar.clone(),
            obligation_farm_user_state.clone(),
            reserve_farm_state.clone(),
            farms_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}

/// CPI: Kamino `withdraw_obligation_collateral_and_redeem_reserve_collateral_v2`.
///
/// V2 carries the farm accounts (farmed reserve — see deposit note).
///
/// Accounts (withdraw group, then farms group):
///   0.  owner (signer, mut) — kamino_authority PDA
///   1.  obligation (mut)
///   2.  lendingMarket
///   3.  lendingMarketAuthority
///   4.  withdrawReserve (mut)
///   5.  reserveLiquidityMint
///   6.  reserveSourceCollateral (mut)
///   7.  reserveCollateralMint (mut)
///   8.  reserveLiquiditySupply (mut)
///   9.  userDestinationLiquidity (mut)
///   10. placeholderUserDestinationCollateral (readonly)
///   11. collateralTokenProgram
///   12. liquidityTokenProgram
///   13. instructionSysvarAccount
///   14. obligationFarmUserState (mut)
///   15. reserveFarmState (mut)
///   16. farmsProgram
///
/// Args: collateral_amount (u64) — ctokens to withdraw (pass u64::MAX to withdraw all).
#[allow(clippy::too_many_arguments)]
pub fn cpi_withdraw<'info>(
    klend_program: &AccountInfo<'info>,
    owner: &AccountInfo<'info>,
    obligation: &AccountInfo<'info>,
    lending_market: &AccountInfo<'info>,
    lending_market_authority: &AccountInfo<'info>,
    withdraw_reserve: &AccountInfo<'info>,
    reserve_liquidity_mint: &AccountInfo<'info>,
    reserve_source_collateral: &AccountInfo<'info>,
    reserve_collateral_mint: &AccountInfo<'info>,
    reserve_liquidity_supply: &AccountInfo<'info>,
    user_destination_liquidity: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    instruction_sysvar: &AccountInfo<'info>,
    obligation_farm_user_state: &AccountInfo<'info>,
    reserve_farm_state: &AccountInfo<'info>,
    farms_program: &AccountInfo<'info>,
    collateral_amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = DISC_WITHDRAW_V2.to_vec();
    data.extend_from_slice(&collateral_amount.to_le_bytes());

    invoke_signed(
        &Instruction {
            program_id: klend_program.key(),
            accounts: vec![
                AccountMeta::new(owner.key(), true),                                  // owner
                AccountMeta::new(obligation.key(), false),                            // obligation
                AccountMeta::new_readonly(lending_market.key(), false),               // lendingMarket
                AccountMeta::new_readonly(lending_market_authority.key(), false),     // lendingMarketAuthority
                AccountMeta::new(withdraw_reserve.key(), false),                      // withdrawReserve
                AccountMeta::new_readonly(reserve_liquidity_mint.key(), false),       // reserveLiquidityMint
                AccountMeta::new(reserve_source_collateral.key(), false),             // reserveSourceCollateral
                AccountMeta::new(reserve_collateral_mint.key(), false),               // reserveCollateralMint
                AccountMeta::new(reserve_liquidity_supply.key(), false),              // reserveLiquiditySupply
                AccountMeta::new(user_destination_liquidity.key(), false),            // userDestinationLiquidity
                AccountMeta::new_readonly(KLEND_PROGRAM_ID, false),                   // placeholderUserDestinationCollateral
                AccountMeta::new_readonly(token_program.key(), false),                // collateralTokenProgram
                AccountMeta::new_readonly(token_program.key(), false),                // liquidityTokenProgram
                AccountMeta::new_readonly(instruction_sysvar.key(), false),           // instructionSysvarAccount
                AccountMeta::new(obligation_farm_user_state.key(), false),            // obligationFarmUserState
                AccountMeta::new(reserve_farm_state.key(), false),                    // reserveFarmState
                AccountMeta::new_readonly(farms_program.key(), false),                // farmsProgram
            ],
            data,
        },
        &[
            owner.clone(),
            obligation.clone(),
            lending_market.clone(),
            lending_market_authority.clone(),
            withdraw_reserve.clone(),
            reserve_liquidity_mint.clone(),
            reserve_source_collateral.clone(),
            reserve_collateral_mint.clone(),
            reserve_liquidity_supply.clone(),
            user_destination_liquidity.clone(),
            token_program.clone(),
            token_program.clone(),
            instruction_sysvar.clone(),
            obligation_farm_user_state.clone(),
            reserve_farm_state.clone(),
            farms_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}
