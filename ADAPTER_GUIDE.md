# Build Your Own Adapter

This guide walks through creating a new yield adapter compatible with the Solana Yield Adapter Standard. By the end you will have a compilable Anchor program that plugs into the Dispatcher and Registry without modifying either.

---

## Prerequisites

- Anchor v1.0.0, Solana v3.1.11 installed (see [README.md](README.md))
- A yield protocol you want to wrap
- The target protocol's on-chain instruction discriminators (see §3)

---

## 1. Copy the Template

```bash
cp -r programs/template programs/my-protocol-adapter
```

Edit `programs/my-protocol-adapter/Cargo.toml`:

```toml
[package]
name = "my-protocol-adapter"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]
name = "my_protocol_adapter"

[dependencies]
anchor-lang = "1.0.0"
anchor-spl = "1.0.0"  # only if you need typed SPL token accounts
```

Add it to the workspace `Cargo.toml`:

```toml
[workspace]
members = [
    # ... existing members ...
    "programs/my-protocol-adapter",
]
```

Add it to `Anchor.toml` under `[programs.localnet]`:

```toml
[programs.localnet]
my_protocol_adapter = "PLACEHOLDER_PUBKEY"
```

Generate a keypair and replace the placeholder:

```bash
solana-keygen new --no-bip39-passphrase --outfile target/deploy/my_protocol_adapter-keypair.json
solana-keygen pubkey target/deploy/my_protocol_adapter-keypair.json
# paste the output into Anchor.toml and into declare_id!() in src/lib.rs
```

---

## 2. Program Structure

```
programs/my-protocol-adapter/
  Cargo.toml
  src/
    lib.rs                  declare_id!, #[program] mod with 4 instructions
    state.rs                MyAdapterPosition account struct
    error.rs                AdapterError enum (use standard codes from SPEC.md §7)
    cpi.rs                  raw CPI helpers and protocol constants
    instructions/
      mod.rs                pub use each instruction module
      initialize_position.rs
      deposit.rs
      withdraw.rs
      current_value.rs
```

---

## 3. Compute Protocol Discriminators

Anchor's discriminator for an instruction named `foo_bar` is:

```
sha256("global:foo_bar")[0..8]
```

In Rust:

```rust
use anchor_lang::solana_program::hash;
let disc = &hash::hash(b"global:foo_bar").to_bytes()[..8];
```

Or compute them ahead of time and hardcode in `cpi.rs`:

```rust
// Example: discriminator for "deposit_reserve_liquidity"
pub const DISC_DEPOSIT: [u8; 8] = [0xf2, 0x23, 0xc6, 0x89, 0x52, 0xe1, 0xf2, 0xb6];
```

To compute from a shell (requires Python 3):

```bash
python3 -c "
import hashlib, sys
name = 'global:' + sys.argv[1]
print(list(hashlib.sha256(name.encode()).digest()[:8]))
" deposit_reserve_liquidity
```

---

## 4. Define the Position State (`state.rs`)

```rust
use anchor_lang::prelude::*;

#[account]
pub struct MyAdapterPosition {
    pub owner: Pubkey,
    pub shares: u64,
    pub authority_bump: u8,
    pub bump: u8,
}

impl MyAdapterPosition {
    pub const SPACE: usize = 8 + 32 + 8 + 1 + 1;
    pub const SEED: &'static [u8] = b"my_pos";
    pub const AUTH_SEED: &'static [u8] = b"my_auth";
}
```

The authority PDA (`MY_AUTH_SEED`) is a virtual PDA that owns any token accounts or protocol accounts held on behalf of the user. It has no lamports of its own; it exists only as a signing key for sub-CPIs via `invoke_signed`.

---

## 5. Implement `initialize_position`

This is called directly by the user, not via the Dispatcher.

```rust
#[derive(Accounts)]
pub struct InitializePosition<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        init, payer = owner,
        space = MyAdapterPosition::SPACE,
        seeds = [MyAdapterPosition::SEED, owner.key().as_ref()],
        bump,
    )]
    pub adapter_position: Account<'info, MyAdapterPosition>,

    /// Virtual signer PDA — no data, used for sub-CPI signing.
    /// CHECK: validated by seeds constraint
    #[account(
        seeds = [MyAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    // ... protocol-specific accounts for one-time setup ...

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePosition>) -> Result<()> {
    let authority_bump = ctx.bumps.authority;
    let owner = ctx.accounts.owner.key();

    let signer_seeds: &[&[&[u8]]] = &[&[
        MyAdapterPosition::AUTH_SEED,
        owner.as_ref(),
        &[authority_bump],
    ]];

    // CPI: open protocol account for authority
    cpi::cpi_open_account(/* ... */, signer_seeds)?;

    let pos = &mut ctx.accounts.adapter_position;
    pos.owner = owner;
    pos.shares = 0;
    pos.authority_bump = authority_bump;
    pos.bump = ctx.bumps.adapter_position;
    Ok(())
}
```

After the user calls this, they must **also** call `dispatcher::initialize_position(adapter_program_id)` to create the Dispatcher-side `UserPosition`. The order matters: adapter's `initialize_position` first.

---

## 6. Implement `deposit`

`deposit` is called by the Dispatcher via `invoke()`. The account struct **must not** declare `owner` as a `Signer` — it is passed as a plain account from `remaining_accounts`.

```rust
#[derive(Accounts)]
pub struct Deposit<'info> {
    /// CHECK: used for PDA seed derivation only — not a signer in this context
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [MyAdapterPosition::SEED, owner.key().as_ref()],
        bump = adapter_position.bump,
    )]
    pub adapter_position: Account<'info, MyAdapterPosition>,

    /// CHECK: authority signer PDA
    #[account(
        seeds = [MyAdapterPosition::AUTH_SEED, owner.key().as_ref()],
        bump = adapter_position.authority_bump,
    )]
    pub authority: UncheckedAccount<'info>,

    // ... token accounts, protocol state, program accounts ...
}

pub fn handler(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, AdapterError::InsufficientFunds);

    // 1. Read current exchange rate from protocol state
    let exchange_rate = cpi::read_exchange_rate(&ctx.accounts.protocol_state)?;

    // 2. Compute shares
    let shares = (amount as u128)
        .checked_mul(SCALE)
        .and_then(|n| n.checked_div(exchange_rate as u128))
        .and_then(|s| u64::try_from(s).ok())
        .ok_or(error!(AdapterError::Overflow))?;

    // 3. Execute protocol CPI
    let owner_key = ctx.accounts.owner.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        MyAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[ctx.accounts.adapter_position.authority_bump],
    ]];
    cpi::cpi_deposit(/* ctx accounts... */, amount, signer_seeds)?;

    // 4. Update shares
    ctx.accounts.adapter_position.shares = ctx.accounts.adapter_position.shares
        .checked_add(shares)
        .ok_or(error!(AdapterError::Overflow))?;

    // 5. REQUIRED: write return data
    set_return_data(&shares.to_le_bytes());
    Ok(())
}
```

---

## 7. Implement `withdraw`

```rust
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    let pos_shares = ctx.accounts.adapter_position.shares;

    let (shares_to_remove, amount_to_withdraw) = if shares == 0 {
        // Withdraw all
        let rate = cpi::read_exchange_rate(&ctx.accounts.protocol_state)?;
        let amount = shares_to_amount(pos_shares, rate)?;
        (pos_shares, amount)
    } else {
        require!(shares <= pos_shares, AdapterError::InsufficientShares);
        let rate = cpi::read_exchange_rate(&ctx.accounts.protocol_state)?;
        let amount = shares_to_amount(shares, rate)?;
        (shares, amount)
    };

    let owner_key = ctx.accounts.owner.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        MyAdapterPosition::AUTH_SEED,
        owner_key.as_ref(),
        &[ctx.accounts.adapter_position.authority_bump],
    ]];

    cpi::cpi_withdraw(/* ctx accounts... */, amount_to_withdraw, signer_seeds)?;

    ctx.accounts.adapter_position.shares = ctx.accounts.adapter_position.shares
        .checked_sub(shares_to_remove)
        .ok_or(error!(AdapterError::InsufficientShares))?;

    // REQUIRED: write return data
    set_return_data(&shares_to_remove.to_le_bytes());
    Ok(())
}
```

**Cooldown protocols:** If the protocol does not immediately transfer funds (e.g. Drift Insurance Fund requires an unbonding period), write `set_return_data(&0u64.to_le_bytes())` and track the pending withdrawal in the position state. The Dispatcher will subtract `shares_removed = 0` from the position, leaving shares unchanged. Track the cooldown in your state; provide a second `claim` instruction the user calls after the cooldown elapses.

---

## 8. Implement `current_value`

```rust
pub fn handler(ctx: Context<CurrentValue>) -> Result<()> {
    let rate = cpi::read_exchange_rate(&ctx.accounts.protocol_state)?;
    let shares = ctx.accounts.adapter_position.shares;

    let value_usdc = shares_to_amount(shares, rate)?;

    set_return_data(&value_usdc.to_le_bytes());
    Ok(())
}
```

This must be view-only — no state mutations. Call with `simulateTransaction`.

---

## 9. Write Raw CPIs (`cpi.rs`)

Example of a raw CPI call:

```rust
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};

pub const PROTOCOL_PROGRAM_ID: Pubkey = pubkey!("...");
pub const DISC_DEPOSIT: [u8; 8] = [0xf2, 0x23, ...]; // precomputed

pub fn cpi_deposit(
    protocol_program: &AccountInfo,
    // ... accounts ...
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&DISC_DEPOSIT);
    data.extend_from_slice(&amount.to_le_bytes());   // Borsh u64

    let ix = Instruction {
        program_id: PROTOCOL_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(signer_account.key(), false),
            // ... in exact order the protocol program expects ...
        ],
        data,
    };

    invoke_signed(
        &ix,
        &[
            signer_account.to_account_info(),
            // ... same accounts as above ...
        ],
        signer_seeds,
    )?;
    Ok(())
}
```

**Tips for finding account order:**
- Read the protocol program's IDL (Anchor programs publish this at the program address).
- Cross-reference with the protocol's TypeScript SDK.
- For non-Anchor programs, read the instruction handler in the protocol source.

---

## 10. Register in the Dispatcher Integration Test

Add `remaining_accounts` arrays for your adapter's deposit, withdraw, and current_value structs to `tests/fork/06_dispatcher.ts` or write a new test file `tests/fork/07_my_adapter.ts` that mirrors the pattern in `01_marginfi.ts`.

---

## 11. Register in the Registry

On devnet:

```bash
npx tsx scripts/register-adapter.ts approve \
  --adapter <YOUR_ADAPTER_PROGRAM_ID> \
  --name "My Protocol USDC" \
  --protocol "MyProtocol" \
  --mint EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
```

On mainnet (after audit):

```bash
ANCHOR_PROVIDER_URL=https://api.mainnet-beta.solana.com \
npx tsx scripts/register-adapter.ts approve \
  --adapter <MAINNET_ADAPTER_PROGRAM_ID> \
  --name "My Protocol USDC" \
  --protocol "MyProtocol" \
  --mint EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
```

---

## 12. Checklist

- [ ] `declare_id!` matches `Anchor.toml` and the deployed keypair
- [ ] All four instructions implemented: `initialize_position`, `deposit`, `withdraw`, `current_value`
- [ ] `deposit` and `withdraw` account structs use `UncheckedAccount` for `owner` (not `Signer`)
- [ ] `deposit` and `withdraw` call `set_return_data` before returning
- [ ] `withdraw` handles `shares == 0` as "withdraw all"
- [ ] Error codes follow the standard ranges in SPEC.md §7
- [ ] `anchor build` succeeds with no warnings about unused variables
- [ ] Fork test written and passing with `npm run test:fork`
- [ ] Adapter registered in Registry (devnet first, mainnet after audit)

---

## Reference: Signer Seeds Pattern

```rust
// In deposit/withdraw handlers, `owner_key` must be captured before borrowing ctx.accounts:
let owner_key = ctx.accounts.owner.key();
let signer_seeds: &[&[&[u8]]] = &[&[
    MyAdapterPosition::AUTH_SEED,
    owner_key.as_ref(),
    &[ctx.accounts.adapter_position.authority_bump],
]];

// Pass to invoke_signed inside cpi::* helpers.
```

## Reference: `withdraw` Lifetime Annotation

If your withdraw instruction passes `remaining_accounts` into a sub-CPI, you need explicit `'info` lifetime:

```rust
// In lib.rs:
pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    instructions::withdraw::handler(ctx, shares)
}

// In instructions/withdraw.rs:
pub fn handler<'info>(ctx: Context<'info, Withdraw<'info>>, shares: u64) -> Result<()> {
    // ...
}
```

This is required by the Anchor v1 borrow checker when you call `.to_vec()` on `ctx.remaining_accounts`.
