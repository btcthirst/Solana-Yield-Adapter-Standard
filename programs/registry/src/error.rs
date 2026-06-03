use anchor_lang::prelude::*;

#[error_code]
pub enum RegistryError {
    #[msg("Signer is not the registry authority")]
    Unauthorized,

    #[msg("Adapter is already registered and active")]
    AlreadyRegistered,

    #[msg("Adapter entry not found or already closed")]
    NotFound,

    #[msg("Cannot close an active entry — revoke first")]
    EntryStillActive,

    #[msg("No authority transfer is pending")]
    NoPendingTransfer,

    #[msg("Signer is not the pending authority")]
    NotPendingAuthority,
}
