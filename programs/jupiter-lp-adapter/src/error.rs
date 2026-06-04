use anchor_lang::prelude::*;

#[error_code]
pub enum AdapterError {
    InsufficientFunds = 0,    // 6000
    InsufficientShares = 1,   // 6001
    SlippageExceeded = 2,     // 6002
    Overflow = 3,             // 6003
    DivisionByZero = 4,       // 6004
    ProtocolError = 100,      // 6100
    PoolDataTooShort = 101,   // 6101
}
