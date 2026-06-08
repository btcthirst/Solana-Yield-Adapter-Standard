# Solana Yield Adapter Standard

A composable, on-chain adapter layer that brings any Solana yield protocol behind a single, uniform interface. Clients call one Dispatcher program; the Dispatcher routes the call to whichever adapter the user chose; adapters translate into native protocol CPIs.

Built for the **Superteam Ukraine Bounty** — Anchor v1.0.0 + Solana v3.1.11.

---

## ⚠️ Mainnet-fork test status (read first)

The Dispatcher, Registry, and **all five adapters compile and run against live mainnet state.** Four of the five adapters pass real mainnet-fork integration tests; the fifth (Drift) is blocked by an upstream change that no adapter code can work around. Two adapters required documented design adaptations because the requested integration does not exist natively on Solana.

| Adapter | Fork test | Status |
|---|---|---|
| MarginFi USDC | `01_marginfi.ts` | ✅ Passes on live mainnet fork |
| Kamino USDC | `02_kamino.ts` | ✅ Passes on live mainnet fork |
| Jupiter LP | `04_jupiter_lp.ts` | ✅ Passes on live mainnet fork |
| Maple syrupUSDC | `05_maple.ts` | ✅ Passes on live mainnet fork *(via Orca swap — see below)* |
| Drift | `03_drift.ts` | ⛔ Externally blocked — skips with on-chain proof |
| Dispatcher (e2e) | `06_dispatcher.ts` | ✅ Passes |

**Drift — physically impossible against the live program.** Drift commented out *every* instruction in its deployed program ([drift-labs/protocol-v2 #2174, "comment out all ixs", 2026-04-01](https://github.com/drift-labs/protocol-v2/pull/2174); [`programs/drift/src/lib.rs`](https://github.com/drift-labs/protocol-v2/blob/master/programs/drift/src/lib.rs) now has 245 commented-out `pub fn` and one custom oracle entrypoint). Any CPI into it — Insurance Fund *or* spot-market — returns `AnchorError 101 (InstructionFallbackNotFound)`, confirmed live via Helius `simulateTransaction`. The adapter is written correctly and its fork test **skips with a documented proof instead of failing**, so it will pass unchanged the moment Drift re-enables its program. Full evidence: [`Docs/troubleshooting/drift-fork-issues.md`](Docs/troubleshooting/drift-fork-issues.md).

**Maple — no native deposit program on Solana.** syrupUSDC is a Chainlink-CCIP bridge token; mint/redeem happens on Ethereum, and the Solana program controlling its pool is the CCIP token-pool (`LockOrBurnTokens`), so a literal "deposit into Maple" CPI cannot exist here. To preserve the uniform USDC-in/USDC-out interface, the adapter routes through the live Orca whirlpool (syrupUSDC/USDC) via `swap_v2`, custodies syrupUSDC, and prices the position against the pool — yield is syrupUSDC NAV appreciation. Details in [Key Design Decisions](#key-design-decisions) and [SPEC.md](SPEC.md).

Both are external protocol realities, not implementation gaps — see the in-depth write-ups in [Key Design Decisions](#key-design-decisions) and [`Docs/troubleshooting/`](Docs/troubleshooting/).

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
| Drift USDC Adapter | `BYT5wbAodWevNJRLnaU2Qe87prHWqycBoZh3oWnCXeY8` |
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
  drift-adapter/    Reference adapter — Drift USDC spot-market lending
  jupiter-lp-adapter/ Reference adapter — Jupiter Perpetuals LP
  maple-adapter/    Reference adapter — Maple syrupUSDC via Orca swap (USDC in/out)
  template/         Blank adapter scaffold (copy to start a new adapter)
tests/
  fork/
    00_setup.ts     Global before-hook: clone mainnet accounts into surfpool
    01_marginfi.ts  MarginFi adapter happy-path fork test
    02_kamino.ts    Kamino adapter happy-path fork test
    03_drift.ts     Drift USDC spot-market adapter fork test (externally blocked — skips)
    04_jupiter_lp.ts Jupiter LP adapter fork test
    05_maple.ts     Maple adapter fork test (USDC↔syrupUSDC via Orca whirlpool)
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
- Solana CLI — [install](https://docs.anza.xyz/cli/install)
- Anchor CLI — [install](https://www.anchor-lang.com/docs/installation)
- Node.js 20+, npm

### Build

```bash
npm install
anchor build
```

### Run mainnet-fork tests

[surfpool](https://github.com/txtx/surfpool) is required. Install the prebuilt
binary (see [surfpool releases](https://github.com/txtx/surfpool/releases) for
other platforms):

```bash
# Linux x64
curl -fsSL https://github.com/txtx/surfpool/releases/latest/download/surfpool-linux-x64.tar.gz \
  | sudo tar -xz -C /usr/local/bin surfpool

# macOS (Homebrew)
brew install txtx/taps/surfpool
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

**The Drift Insurance Fund adapter cannot be built — Drift commented out every instruction in its program.** This is not a design choice; the requested integration is *physically impossible* against the live program, and the proof is in Drift's own source.

Drift's deployed program (`dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH`) has had its **entire `#[program]` instruction set commented out** upstream. In [`drift-labs/protocol-v2`](https://github.com/drift-labs/protocol-v2) `master`, [`programs/drift/src/lib.rs`](https://github.com/drift-labs/protocol-v2/blob/master/programs/drift/src/lib.rs) (2258 lines) now contains **exactly one active `pub fn`** (`program_entry`, a custom entrypoint serving only two native oracle ops under the `FF FF FF FF` prefix) and **245 commented-out `pub fn`** instructions. The last commit to touch the file is literally [**"comment out all ixs" (#2174)**](https://github.com/drift-labs/protocol-v2/pull/2174), 2026-04-01.

**Every Insurance Fund instruction is explicitly commented out** (verified line numbers in that file):

| Instruction | Line |
|---|---|
| `settle_revenue_to_insurance_fund` | 736 |
| `initialize_insurance_fund_stake` | 796 |
| `add_insurance_fund_stake` | 803 |
| `request_remove_insurance_fund_stake` | 811 |
| `cancel_request_remove_insurance_fund_stake` | 819 |
| `remove_insurance_fund_stake` | 826 |
| `begin_insurance_fund_swap` / `end_insurance_fund_swap` | 841 / 850 |
| `admin_withdraw_from_insurance_fund_vault` | 866 |
| `deposit_into_insurance_fund_stake` | 874 |
| `update_insurance_fund_unstaking_period` | 1272 |

Because the anchor dispatcher now has zero registered instructions, **any** CPI into the program — IF stake or anything else — returns `AnchorError 101 (InstructionFallbackNotFound)`, byte-identically to a bogus discriminator (confirmed live on mainnet via Helius `simulateTransaction`). The program is also invoked by no one on mainnet (0 direct invocations observed over ~88 h; canonical accounts are *not* migrated to a new program ID). No adapter code can work around this.

To still demonstrate the correct CPI model, the adapter is implemented as **Drift USDC spot-market lending** (`initialize_user`/`deposit`/`withdraw`, `market_index = 0`) — but those spot-market instructions live in the *same* commented-out `#[program]`, so they are blocked identically. Its fork test (`03_drift.ts`) probes the program live and **skips with a documented proof** rather than failing, so CI stays green; it will pass unchanged the moment Drift re-enables its program. Full evidence: [`Docs/troubleshooting/drift-fork-issues.md`](Docs/troubleshooting/drift-fork-issues.md). **The other four adapters pass real mainnet-fork tests.**

**Design note — Maple adapter (USDC in/out via Orca).** Maple has **no native deposit program on Solana** — syrupUSDC is a Chainlink-CCIP bridge token whose mint/redeem happens on Ethereum (the Solana program controlling its pool is the CCIP token-pool, instruction `LockOrBurnTokens`), so a literal "deposit into Maple" CPI is impossible here. To preserve the uniform USDC-in/USDC-out interface, the adapter routes through the live Orca whirlpool (`6fteKNvM…`, syrupUSDC/USDC): `deposit` swaps USDC → syrupUSDC and custodies it; `withdraw` swaps back to USDC; `current_value` prices the position against the pool's `sqrt_price`. Yield is syrupUSDC's NAV appreciation. Entry/exit pay the 0.01% pool fee + price impact; min-out is derived on-chain (1% slippage floor) so the interface stays `deposit(amount)`. Swaps use Orca `swap_v2`. See SPEC.md §3 for details.

---

## CI

GitHub Actions workflow (`.github/workflows/test.yml`) runs on every push to `main`/`develop`:

1. Install Rust stable, Solana CLI, Anchor CLI, surfpool
2. `anchor build`
3. Start surfpool daemon pointing at `HELIUS_RPC_URL` secret
4. `npm run test:fork`

---

## Contributing

To add a new adapter, follow [ADAPTER_GUIDE.md](ADAPTER_GUIDE.md). The interface contract is documented in [SPEC.md](SPEC.md).

---

## License

[MIT](LICENSE) — free to use, fork, and build your own adapters on top of this standard.
