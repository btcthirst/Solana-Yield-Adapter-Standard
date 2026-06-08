// Scaffold module: parameters are intentionally named (not `_`-prefixed) to
// document what each placeholder helper should receive. Silence the unused
// warnings until you fill in the real logic.
#![allow(unused_variables)]

use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::Instruction,
        program::invoke_signed,
    },
};

// ── Protocol constants ────────────────────────────────────────────────────────
// Replace with the real protocol program ID.
pub const PROTOCOL_PROGRAM_ID: Pubkey = pubkey!("11111111111111111111111111111111");

// Replace with sha256("global:{instruction_name}")[0..8] for each instruction.
// Python: python3 -c "import hashlib,sys; print(list(hashlib.sha256(('global:'+sys.argv[1]).encode()).digest()[:8]))" deposit
pub const DISC_DEPOSIT:  [u8; 8] = [0x00; 8]; // TODO
pub const DISC_WITHDRAW: [u8; 8] = [0x00; 8]; // TODO

// ── Exchange rate helpers ─────────────────────────────────────────────────────

/// Read the current exchange rate from protocol state.
/// Returns a scaled integer; scale factor is protocol-specific.
pub fn read_exchange_rate(protocol_state: &AccountInfo) -> Result<u64> {
    // TODO: read the protocol account data and extract the exchange rate.
    // Example (offset 88, 8 bytes LE):
    //   let data = protocol_state.try_borrow_data()?;
    //   Ok(u64::from_le_bytes(data[88..96].try_into().unwrap()))
    Ok(1_000_000) // placeholder: 1.0 at 6-decimal scale
}

/// Convert asset lamports → internal shares.
pub fn amount_to_shares(amount: u64, rate: u64) -> Option<u64> {
    // Adapt the formula to match the protocol's share model.
    // Example (I80F48 math for MarginFi):
    //   (amount as u128).checked_mul(1u128 << 48)?.checked_div(rate as u128)?.try_into().ok()
    Some(amount) // placeholder: 1 share = 1 lamport
}

/// Convert internal shares → asset lamports.
pub fn shares_to_amount(shares: u64, rate: u64) -> Option<u64> {
    // Inverse of amount_to_shares.
    Some(shares) // placeholder
}

// ── CPI helpers ───────────────────────────────────────────────────────────────

pub fn cpi_deposit<'info>(
    protocol_program: &AccountInfo<'info>,
    // TODO: add the remaining AccountInfo params the protocol needs
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&DISC_DEPOSIT);
    data.extend_from_slice(&amount.to_le_bytes());

    let ix = Instruction {
        program_id: PROTOCOL_PROGRAM_ID,
        accounts: vec![
            // TODO: AccountMeta entries in exact protocol-expected order
        ],
        data,
    };

    invoke_signed(
        &ix,
        &[
            protocol_program.clone(),
            // TODO: same accounts as above
        ],
        signer_seeds,
    )?;
    Ok(())
}

pub fn cpi_withdraw<'info>(
    protocol_program: &AccountInfo<'info>,
    // TODO: remaining AccountInfo params
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&DISC_WITHDRAW);
    data.extend_from_slice(&amount.to_le_bytes());

    let ix = Instruction {
        program_id: PROTOCOL_PROGRAM_ID,
        accounts: vec![
            // TODO: AccountMeta entries
        ],
        data,
    };

    invoke_signed(
        &ix,
        &[
            protocol_program.clone(),
            // TODO
        ],
        signer_seeds,
    )?;
    Ok(())
}
