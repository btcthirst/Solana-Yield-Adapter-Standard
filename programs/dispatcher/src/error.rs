use anchor_lang::prelude::*;

#[error_code]
pub enum DispatcherError {
    #[msg("Insufficient shares for withdrawal")]
    InsufficientShares = 1,

    #[msg("Arithmetic overflow in shares calculation")]
    Overflow = 3,

    #[msg("Adapter is not registered in the Registry")]
    AdapterNotRegistered = 200,

    #[msg("Adapter has been permanently revoked")]
    AdapterRevoked = 201,

    #[msg("Adapter is temporarily paused")]
    AdapterPaused = 202,

    #[msg("Adapter did not return expected data")]
    AdapterError = 204,
}
