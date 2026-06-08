use anchor_lang::prelude::*;

// Standard error codes — keep these consistent across all adapters (see SPEC.md §7).
#[error_code]
pub enum AdapterError {
    InsufficientFunds = 0,  // 6000
    InsufficientShares = 1, // 6001
    SlippageExceeded = 2,   // 6002
    Overflow = 3,           // 6003
    ProtocolError = 100,    // 6100
    CooldownActive = 101,   // 6101
    OracleStale = 103,      // 6103
}
