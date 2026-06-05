# Solana Yield Adapter Standard

A composable, on-chain adapter layer that brings any Solana yield protocol behind a single, uniform interface. Clients call one Dispatcher program; the Dispatcher routes the call to whichever adapter the user chose; adapters translate into native protocol CPIs.

Built for the **Superteam Ukraine Bounty** — Anchor v1.0.0 + Solana v3.1.11.

> **Toolchain note:** The bounty spec lists Anchor 0.31.1 / Solana 2.2.20. This implementation uses Anchor 1.0.0 / Solana 3.1.11 — the latest stable releases at submission time. Anchor 1.0.0 introduced breaking changes (`Context` lifetime rules, `UncheckedAccount` patterns) that required adapted patterns documented in the codebase; the interface contract in SPEC.md is identical.

---

## Architecture

```
Client TX
   │
   ▼
┌──────────────────────────────────┐
│         Dispatcher Program        │  F6QyZM6rb5i1bDsW9gQMrPBVzZeEfSRbR2JDzDQTJuQ1
│  initialize_position             │
│  deposit(amount)                 │
│  withdraw(shares)                │
│  current_value()                 │
│  close_position()                │
└────────────┬─────────────────────┘
             │ checks RegistryEntry (active, not paused)
             ▼
┌──────────────────────────────────┐
│         Registry Program          │  4NP3DgbM7JJDBQEiU9ojUJ3yYnCoEnbBCsACfQz32xdB
│  RegistryState (authority, ...)  │
│  RegistryEntry per adapter       │
└──────────────────────────────────┘
             │ CPI (invoke, remaining_accounts)
             ▼
┌──────────────────────────────────┐
│       Adapter Program             │  implements AdapterInterface
│  initialize_position             │
│  deposit → set_return_data       │
│  withdraw → set_return_data      │
│  current_value → set_return_data │
└────────────┬─────────────────────┘
             │ raw CPI (no crate dependency)
             ▼
       Yield Protocol
  (MarginFi / Kamino / Drift / Jupiter / Maple)
```

**Data flow for `deposit`:**
1. Dispatcher verifies `RegistryEntry.is_active && !is_paused`
2. Dispatcher CPIs the adapter via `invoke()`, passing `remaining_accounts` as adapter account list
3. Adapter executes protocol CPI, computes `shares_received`, calls `set_return_data(&shares.to_le_bytes())`
4. Dispatcher reads return data via `get_return_data()`, adds shares to `UserPosition`

---

## Program IDs

All programs are deployed on **devnet**. Localnet IDs are identical (same keypairs).

| Program | ID |
|---|---|
| Dispatcher | `F6QyZM6rb5i1bDsW9gQMrPBVzZeEfSRbR2JDzDQTJuQ1` |
| Registry | `4NP3DgbM7JJDBQEiU9ojUJ3yYnCoEnbBCsACfQz32xdB` |
| MarginFi USDC Adapter | `47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz` |
| Kamino USDC Adapter | `5ksJ5dU6jAoZaUnpcXtGN69xXewcRcGLTBisQHSkwc44` |
| Drift IF Adapter | `BYT5wbAodWevNJRLnaU2Qe87prHWqycBoZh3oWnCXeY8` |
| Jupiter LP Adapter | `7JVMN1WEVmXGFdAu5AQsGFfxEAjoL2uD79hEzeo9115E` |
| Maple syrupUSDC Adapter | `EuffaJ2ccu1PnppDd5rTBxPvFXA4u8YQKDj6DyqsyVot` |

Registry authority: `DeDQoza7kLWuNWYEgDqxB9YzqhYh3Tw62cufo8JygpSy`

All five adapters are approved and active in the devnet Registry. Verify:

```bash
ANCHOR_PROVIDER_URL=https://api.devnet.solana.com \
npx tsx scripts/register-adapter.ts list
```

---

## Repository Layout

```
programs/
  dispatcher/       Dispatcher program
  registry/         On-chain Registry program
  marginfi-adapter/ Reference adapter — MarginFi USDC lending
  kamino-adapter/   Reference adapter — Kamino USDC lending
  drift-adapter/    Reference adapter — Drift Insurance Fund
  jupiter-lp-adapter/ Reference adapter — Jupiter Perpetuals LP
  maple-adapter/    Reference adapter — Maple Finance syrupUSDC
  template/         Blank adapter scaffold (copy to start a new adapter)
tests/
  fork/
    00_setup.ts     Global before-hook: clone mainnet accounts into surfpool
    01_marginfi.ts  MarginFi adapter happy-path fork test
    02_kamino.ts    Kamino adapter happy-path fork test
    03_drift.ts     Drift IF adapter fork test (request + cooldown)
    04_jupiter_lp.ts Jupiter LP adapter fork test
    05_maple.ts     Maple syrupUSDC adapter fork test
    06_dispatcher.ts End-to-end Dispatcher integration test + negative cases
scripts/
  register-adapter.ts  CLI: register/update/pause/revoke adapters in Registry
SPEC.md            Adapter Standard Specification (formal interface)
ADAPTER_GUIDE.md   Step-by-step guide: build your own adapter
```

---

## Quick Start

### Prerequisites

- Rust stable (`rustup default stable`)
- Solana CLI v3.1.11 — [install](https://docs.anza.xyz/cli/install)
- Anchor CLI v1.0.0 — `cargo install --git https://github.com/coral-xyz/anchor anchor-cli --tag v1.0.0 --locked`
- Node.js 20+, npm

### Build

```bash
npm install
anchor build
```

### Run mainnet-fork tests

[surfpool](https://github.com/txtx/surfpool) is required. Install:

```bash
cargo install surfpool --locked
```

Export a mainnet RPC endpoint (Helius, Triton, or any full node):

```bash
export SURFPOOL_DATASOURCE_RPC_URL=https://mainnet.helius-rpc.com/?api-key=YOUR_KEY
```

Start surfpool and run the test suite:

```bash
NO_DNA=1 surfpool start --network mainnet --no-tui --rpc-port 8899 \
  --datasource-rpc-url "$SURFPOOL_DATASOURCE_RPC_URL" --daemon

npm run test:fork
```

Or let Anchor manage surfpool automatically:

```bash
anchor test
```

### Register an adapter on devnet

```bash
# Requires ANCHOR_WALLET and ANCHOR_PROVIDER_URL=https://api.devnet.solana.com
npx tsx scripts/register-adapter.ts approve \
  --adapter <ADAPTER_PROGRAM_ID> \
  --name "My Protocol USDC" \
  --protocol "MyProtocol" \
  --mint EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
```

---

## Key Design Decisions

**Raw CPI, no crate dependencies.** Adapters call underlying protocols using `anchor_lang::solana_program::program::invoke` with manually built instruction data (discriminator + args serialized as Borsh). This keeps adapter compile times short and removes the risk of version conflicts with upstream protocol crates.

**Shares as a unit of account.** Each adapter maintains its own share<→>asset exchange rate. The Dispatcher stores `UserPosition.shares`; the adapter stores protocol-specific share counts. `current_value` translates shares back to USDC lamports at query time.

**`remaining_accounts` for adapter accounts.** The Dispatcher passes adapter instruction accounts via `ctx.remaining_accounts` so the adapter struct is entirely self-contained and the Dispatcher does not need to know adapter account layouts.

**Registry is the source of trust.** The Dispatcher reads `RegistryEntry.is_active` (revoked check) and `is_paused` before every CPI. Constraint checks happen before the adapter CPI, so invalid calls are rejected cheaply.

**Known limitation — Maple adapter.** The Maple Finance adapter accepts **syrupUSDC** (Maple's yield-bearing token) directly rather than USDC. Users must acquire syrupUSDC externally — via an Orca/Jupiter swap on Solana or via Chainlink CCIP from Ethereum — before calling `deposit`. This breaks the uniform USDC-in/USDC-out abstraction that the other four adapters provide. See SPEC.md §3 for details.

---

## CI

GitHub Actions workflow (`.github/workflows/test.yml`) runs on every push to `main`/`develop`:

1. Install Rust stable, Solana CLI v3.1.11, Anchor CLI v1.0.0, surfpool
2. `anchor build`
3. Start surfpool daemon pointing at `HELIUS_RPC_URL` secret
4. `npm run test:fork`

---

## Contributing

To add a new adapter, follow [ADAPTER_GUIDE.md](ADAPTER_GUIDE.md). The interface contract is documented in [SPEC.md](SPEC.md).
