use anchor_lang::prelude::*;

#[error_code]
pub enum AdapterError {
    #[msg("Cooldown period has not elapsed — call again after ready_at")]
    CooldownActive,
    #[msg("No pending withdrawal to complete — initiate request first")]
    NoPendingWithdrawal,
    #[msg("Insufficient shares in position")]
    InsufficientShares,
    #[msg("Protocol-side error or unexpected account state")]
    ProtocolError,
    #[msg("Arithmetic overflow in value computation")]
    Overflow,
}
