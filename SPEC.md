# Solana Yield Adapter Standard — Specification

**Version:** 1.0.0
**Runtime:** Anchor v1.0.0, Solana v3.1.11

---

## 1. Overview

This document defines the on-chain interface that every yield adapter program must implement to be compatible with the Dispatcher and Registry programs.

An **adapter** is an Anchor program that wraps a single yield protocol and asset pair. It exposes four instructions (`initialize_position`, `deposit`, `withdraw`, `current_value`) and communicates results back to the Dispatcher via `set_return_data`.

---

## 2. Programs

### 2.1 Registry

**Program ID:** `4NP3DgbM7JJDBQEiU9ojUJ3yYnCoEnbBCsACfQz32xdB`

Maintains the authoritative list of approved adapters.

#### Accounts

**`RegistryState`** — singleton PDA
- Seeds: `[b"registry_state"]` @ Registry
- Space: 8 + 32 + 32 + 1 = 73 bytes

| Field | Type | Description |
|---|---|---|
| `authority` | `Pubkey` | Can approve/revoke adapters |
| `pending_authority` | `Pubkey` | Proposed next authority (2-step transfer) |
| `bump` | `u8` | PDA bump seed |

**`RegistryEntry`** — one per approved adapter
- Seeds: `[b"registry_entry", adapter_program_id]` @ Registry
- Space: 245 bytes

| Field | Type | Description |
|---|---|---|
| `adapter_program_id` | `Pubkey` | The adapter program |
| `name` | `[u8; 64]` | Null-padded UTF-8 human-readable name |
| `protocol` | `[u8; 64]` | Null-padded UTF-8 protocol name |
| `asset_mint` | `Pubkey` | Base asset mint (e.g. USDC) |
| `version` | `u16` | Incremented on `update_adapter` |
| `is_active` | `bool` | `false` = permanently revoked |
| `is_paused` | `bool` | `true` = temporarily disabled |
| `approved_at` | `i64` | Unix timestamp |
| `approved_by` | `Pubkey` | Authority that approved |

#### Instructions

| Instruction | Authority | Description |
|---|---|---|
| `initialize_registry(authority)` | payer | One-time setup |
| `approve_adapter(id, name, protocol, mint)` | authority | Creates RegistryEntry |
| `revoke_adapter(id)` | authority | Sets `is_active = false` |
| `pause_adapter(id)` | authority | Sets `is_paused = true` |
| `resume_adapter(id)` | authority | Sets `is_paused = false` |
| `update_adapter(id, name?, protocol?, mint?)` | authority | Updates metadata, bumps version |
| `close_registry_entry(id)` | authority | Closes revoked entry, returns rent |
| `propose_authority(new)` | authority | Step 1 of authority transfer |
| `accept_authority()` | pending_authority | Step 2 of authority transfer |

#### Error Codes

Registry errors use Anchor's default sequential offsets from base 6000:

| Code | Name | Condition |
|---|---|---|
| 6000 | `Unauthorized` | Signer is not the registry authority |
| 6001 | `AlreadyRegistered` | Adapter already has an active RegistryEntry |
| 6002 | `NotFound` | RegistryEntry does not exist or is already closed |
| 6003 | `EntryStillActive` | Cannot close entry — revoke it first |
| 6004 | `NoPendingTransfer` | `accept_authority` called with no pending transfer |
| 6005 | `NotPendingAuthority` | Signer is not the pending authority |

---

### 2.2 Dispatcher

**Program ID:** `F6QyZM6rb5i1bDsW9gQMrPBVzZeEfSRbR2JDzDQTJuQ1`

Routes user calls to the correct adapter after verifying registry status.

#### Accounts

**`UserPosition`** — one per (owner, adapter) pair
- Seeds: `[b"position", owner, adapter_program_id]` @ Dispatcher
- Space: 81 bytes

| Field | Type | Description |
|---|---|---|
| `owner` | `Pubkey` | Position owner |
| `adapter` | `Pubkey` | Adapter program ID |
| `shares` | `u64` | Share balance (unit defined by adapter) |

#### Instructions

**`initialize_position(adapter: Pubkey)`**
- Creates `UserPosition` for `(owner, adapter)`.
- Does **not** call the adapter — the adapter's own `initialize_position` must be called separately (and first) to create adapter-side state.

**`deposit(amount: u64)`**
- Verifies registry (`is_active && !is_paused`).
- Calls adapter's `deposit(amount)` via `invoke()`.
- Reads `shares_received` (u64 LE) from return data.
- Adds shares to `UserPosition.shares`.

**`withdraw(shares: u64)`**
- Verifies registry.
- `shares == 0` means "withdraw all".
- Calls adapter's `withdraw(shares)`.
- Reads `shares_removed` from return data; subtracts from `UserPosition.shares`.
- Note: cooldown protocols (e.g. Drift IF) may return `shares_removed = 0` when a withdrawal is only queued, not executed immediately.

**`close_position(adapter: Pubkey)`**
- Requires `UserPosition.shares == 0`.
- Closes the PDA and returns rent to owner.

**`current_value()`**
- Calls adapter's `current_value()`.
- Re-propagates the u64 return data (USDC lamports) to the outer transaction.
- Intended for use with `simulateTransaction` — does not submit a tx.

#### Dispatcher Error Codes

Dispatcher errors use explicit offsets (non-sequential). The `AdapterRevoked`/`AdapterPaused` checks originate in the Dispatcher — they are thrown during account constraint validation, before any CPI is made:

| Code (offset) | Name | Condition |
|---|---|---|
| 6001 (1) | `InsufficientShares` | `UserPosition.shares < requested shares` |
| 6003 (3) | `Overflow` | Arithmetic overflow in share accounting |
| 6201 (201) | `AdapterRevoked` | `RegistryEntry.is_active == false` |
| 6202 (202) | `AdapterPaused` | `RegistryEntry.is_paused == true` |
| 6204 (204) | `AdapterError` | Adapter returned no data or invalid return data |
| 6205 (205) | `Unauthorized` | Signer is not the owner of the `UserPosition` (`close_position`) |

> **Missing adapter.** When no `RegistryEntry` PDA exists for the adapter, the
> `registry_entry` account is loaded as a typed `Account<RegistryEntry>`, so
> Anchor fails deserialization first with its built-in **`AccountNotInitialized`
> (3012)** — *not* a Dispatcher code. `AdapterNotRegistered` (6200) is defined in
> the enum but reserved; clients detecting an unregistered adapter should match
> on Anchor error `3012`.

---

### 2.3 Adapter Interface

Every adapter program **must** implement the following four instructions. The instruction names and parameter signatures are fixed; account structures are adapter-specific.

#### 2.3.1 `initialize_position()`

- **Called by:** the user directly (not via Dispatcher).
- **Effect:** creates adapter-side position PDA and performs any one-time protocol setup (e.g. opening a lending account, registering for insurance fund staking).
- **Return data:** none required.
- **Must:** be idempotent or guard against double-init — calling it twice should either succeed (no-op) or fail gracefully.

#### 2.3.2 `deposit(amount: u64)`

- **Called by:** Dispatcher via `invoke()`.
- **Accounts:** passed as `remaining_accounts` from the Dispatcher transaction, in the exact order defined by the adapter's Anchor `#[derive(Accounts)]` struct.
- **Effect:** deposits `amount` (asset lamports) into the underlying protocol. Updates internal share balance.
- **Return data:** must call `set_return_data(&shares_received.to_le_bytes())` before returning.
  - `shares_received: u64`, little-endian.
- **Constraints:**
  - `amount == 0` should error with `InsufficientFunds` (6000).
  - Arithmetic overflows must error with `Overflow` (6003).

#### 2.3.3 `withdraw(shares: u64)`

- **Called by:** Dispatcher via `invoke()`.
- **Accounts:** passed as `remaining_accounts`.
- **Effect:** withdraws the given number of shares from the protocol. May be immediate or deferred (cooldown).
- **Return data:** must call `set_return_data(&shares_removed.to_le_bytes())`.
  - For immediate withdrawals: `shares_removed == shares` (or the full position if `shares == 0`).
  - For queued/cooldown withdrawals: `shares_removed == 0` until cooldown elapses.
- **Constraints:**
  - `shares > adapter_position.shares` must error with `InsufficientShares` (6001).

#### 2.3.4 `current_value()`

- **Called by:** Dispatcher via `invoke()` inside `simulateTransaction`.
- **Accounts:** passed as `remaining_accounts`. Typically only needs the adapter position PDA and any oracle/market accounts required to compute the exchange rate.
- **Effect:** reads current share→asset exchange rate from on-chain protocol state. No state mutations.
- **Return data:** must call `set_return_data(&value_usdc.to_le_bytes())`.
  - `value_usdc: u64` — current USDC lamport value of the position.

---

## 3. Share Accounting

Each adapter defines its own internal share unit. The Dispatcher tracks only the `u64 shares` reported by the adapter. The relationship between shares and asset value is protocol-specific:

| Adapter | Share Unit |
|---|---|
| MarginFi | `amount × 2^48 / asset_share_value` (I80F48 math) |
| Kamino | cToken count (kUSDC collateral tokens in obligation) |
| Drift | `if_shares` (u128, tracked as u64 — must not exceed u64::MAX) |
| Jupiter LP | JLP token lamports held in authority ATA |
| Maple | syrupUSDC lamports held in authority ATA (1 share = 1 lamport) |

`current_value()` must always compute the live USDC value from the current exchange rate, not a cached value.

> **Known limitation — Maple adapter:** Unlike the other four adapters, the Maple adapter accepts **syrupUSDC** directly rather than USDC. syrupUSDC is Maple's yield-bearing token and must be acquired externally before calling `deposit` (via Orca/Jupiter swap on Solana, or via Chainlink CCIP bridge from Ethereum). This breaks the uniform USDC-in/USDC-out abstraction for this adapter. A future version could wrap an Orca CPI to automate the swap; the current implementation prioritises correctness of the custodial model over interface uniformity.
>
> **NAV oracle — Maple `current_value`:** The NAV is read from the canonical
> `MAPLE_POOL_STATE` account (identity enforced by an `address` constraint;
> passing any other account fails with `InvalidPoolState`). The account's byte
> layout (`total_assets_usdc @ 8`, `total_syrup_supply @ 16`) is **not yet
> verified against mainnet**. When the data cannot be parsed into a sane NAV
> (account not yet populated by CCIP, or unverified layout), `current_value`
> returns a **conservative 1.0 floor** — syrupUSDC is yield-bearing so true NAV
> is always ≥ 1.0, meaning the floor never *overstates* value — and emits a
> `msg!` log (`"NAV oracle unavailable — using conservative 1.0 floor"`) so a
> client can distinguish a fallback from a verified reading. Verifying the
> on-chain offsets and removing the fallback is tracked as follow-up work.

---

## 4. Return Data Contract

The Dispatcher depends on return data written by the adapter. The contract:

| Instruction | Written by | Format |
|---|---|---|
| `deposit` | adapter | `u64`, little-endian — shares received |
| `withdraw` | adapter | `u64`, little-endian — shares removed (0 if cooldown) |
| `current_value` | adapter | `u64`, little-endian — USDC lamport value |
| `initialize_position` | optional | not read by Dispatcher |

`set_return_data` must be called unconditionally before `Ok(())`. If the call returns before writing return data, the Dispatcher will error with `AdapterError` (6204).

---

## 5. Signer Propagation

The Dispatcher calls adapters using `invoke()` (not `invoke_signed`). This propagates `is_signer = true` for any account that was a signer in the original transaction. In practice:

- **`owner` in adapter structs** — declared as `UncheckedAccount` (not `Signer`). The Dispatcher does not pass a signer; `owner` is read-only for PDA seed derivation.
- **User token accounts** — the user's wallet signature propagates through `invoke()`, allowing SPL token transfers from the user's ATA inside adapter logic.
- **Adapter authority PDAs** — adapters use `invoke_signed` for sub-CPIs that need PDA signatures; the adapter's own signer seeds are provided internally.

---

## 6. PDA Reference

All PDAs use canonical Anchor derivation (`find_program_address`).

| PDA | Seeds | Program |
|---|---|---|
| RegistryState | `[b"registry_state"]` | Registry |
| RegistryEntry | `[b"registry_entry", adapter_program_id]` | Registry |
| UserPosition | `[b"position", owner, adapter_program_id]` | Dispatcher |
| Adapter position | adapter-defined | Adapter |
| Adapter authority | adapter-defined (e.g. `[b"mfi_auth", owner]`) | Adapter |

---

## 7. Error Codes

Anchor assigns program-local error codes independently per program, all starting from base **6000 + offset**. There are no cross-program numeric ranges — Registry, Dispatcher, and every adapter each have their own independent error space that happens to start at 6000.

The practical consequence: when a transaction fails, inspect **which program** returned the error (`logs` field) to determine whether a code like `6001` means `InsufficientShares` (adapter) or `AlreadyRegistered` (Registry).

### Adapter standard codes (all adapters must use these offsets)

Standardising adapter error offsets allows clients to interpret errors from any adapter uniformly without knowing the specific adapter program.

| Offset | Value | Name | Meaning |
|---|---|---|---|
| 0 | 6000 | `InsufficientFunds` | `amount == 0` on deposit |
| 1 | 6001 | `InsufficientShares` | Not enough shares to withdraw |
| 2 | 6002 | `SlippageExceeded` | Protocol returned fewer assets than minimum |
| 3 | 6003 | `Overflow` | Arithmetic overflow in share computation |
| 100 | 6100 | `ProtocolError` | Unexpected protocol state or wrong account |
| 101 | 6101 | `CooldownActive` | Withdrawal is in cooldown, re-call later |
| 103 | 6103 | `OracleStale` | Oracle price is too old to use |

### Dispatcher error codes

| Offset | Value | Name | Meaning |
|---|---|---|---|
| 1 | 6001 | `InsufficientShares` | `UserPosition.shares < requested shares` |
| 3 | 6003 | `Overflow` | Arithmetic overflow in share accounting |
| 200 | 6200 | `AdapterNotRegistered` | *Reserved* — a missing entry surfaces as Anchor `AccountNotInitialized` (3012) |
| 201 | 6201 | `AdapterRevoked` | `is_active == false` |
| 202 | 6202 | `AdapterPaused` | `is_paused == true` |
| 204 | 6204 | `AdapterError` | Adapter returned no data or invalid return data |
| 205 | 6205 | `Unauthorized` | Signer is not the `UserPosition` owner (`close_position`) |

### Registry error codes

| Offset | Value | Name | Meaning |
|---|---|---|---|
| 0 | 6000 | `Unauthorized` | Signer is not the registry authority |
| 1 | 6001 | `AlreadyRegistered` | *Reserved* — duplicate approval is rejected by Anchor `init` (account already in use) |
| 2 | 6002 | `NotFound` | *Reserved* — a missing entry surfaces as Anchor `AccountNotInitialized` (3012) |
| 3 | 6003 | `EntryStillActive` | Revoke before closing |
| 4 | 6004 | `NoPendingTransfer` | No authority transfer in progress |
| 5 | 6005 | `NotPendingAuthority` | Signer is not the pending authority |

> `AlreadyRegistered` and `NotFound` are defined in the `RegistryError` enum for
> completeness but are not thrown by the current handlers: adapter (de)registration
> is gated by Anchor's `init` / seeded-`Account` loading, which produce the built-in
> errors noted above.

---

## 8. Instruction Discriminator Convention

Adapters call underlying protocols using raw instruction data assembled in `cpi.rs`. Discriminators follow the Anchor convention:

```
discriminator = sha256("global:{instruction_name}")[0..8]
```

This allows cross-program calls without importing protocol crates.

---

## 9. Versioning

The Registry `RegistryEntry.version` field is incremented by `update_adapter`. Clients reading adapter metadata should check `version` to detect stale cached data.

The adapter interface itself is versioned by this document. Breaking changes to the four-instruction interface will increment the spec version and require a new Registry entry with an updated adapter program ID.
