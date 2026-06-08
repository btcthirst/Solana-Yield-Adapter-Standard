use anchor_lang::prelude::*;

#[error_code]
pub enum AdapterError {
    #[msg("Deposit amount must be greater than zero")]
    ZeroAmount,
    #[msg("No active deposit to withdraw")]
    InsufficientShares,
    #[msg("Arithmetic overflow in value computation")]
    Overflow,
}
