/**
 * Global test setup — runs before all fork tests (alphabetical order).
 *
 * Anchor v1.0.0 does not pass `snapshot =` from [surfpool] in Anchor.toml
 * to surfpool's --snapshot CLI flag. This file works around that by fetching
 * required mainnet state accounts from SURFPOOL_DATASOURCE_RPC_URL and
 * injecting them into the running surfpool instance via surfnet_setAccount.
 *
 * Only non-executable state accounts are cloned here (mints, market state,
 * vaults, etc.). Executable program accounts (MarginFi, KLend, Drift, Jupiter)
 * are loaded lazily by surfpool's fork mechanism during CPI execution.
 *
 * When SURFPOOL_DATASOURCE_RPC_URL is not set, this is a no-op and each
 * individual test will gracefully skip.
 */

import { FORK_REQUIRED } from "../utils/forkGuard";

const MAINNET_RPC = process.env.SURFPOOL_DATASOURCE_RPC_URL;
const LOCAL_RPC   = "http://127.0.0.1:8899";

// State accounts required by the fork tests, grouped by adapter.
// Keys map to a human-readable label for log output.
const ACCOUNTS_TO_CLONE: Record<string, string> = {
  // ── Common ────────────────────────────────────────────────────────────────
  "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": "USDC mint",

  // ── MarginFi ──────────────────────────────────────────────────────────────
  "2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB": "MarginFi USDC bank",

  // ── Kamino ────────────────────────────────────────────────────────────────
  "D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59": "Kamino USDC reserve",
  "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF": "Kamino main lending market",

  // ── Drift ─────────────────────────────────────────────────────────────────
  "5zpq7DvB6UdFFvpmBPspGPNfUGoBRRCE2HHg5u3gxcsN": "Drift state",
  "6gMq3mRCKf8aP3ttTyYhuijVZ2LGi14oDsBbkgubfLB3": "Drift USDC spot market",
  "2CqkQvYxp9Mq4PqLvAQ1eryYxebUh4Liyn5YMDtXsYci": "Drift USDC IF vault",

  // ── Jupiter LP ────────────────────────────────────────────────────────────
  "5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq": "Jupiter JLP pool",
  "27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4": "Jupiter JLP mint",
  "G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa": "Jupiter USDC custody",

  // ── Maple ─────────────────────────────────────────────────────────────────
  "AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj": "syrupUSDC mint",
  "HrTBpF3LqSxXnjnYdR4htnBLyMHNZ6eNaDZGPundvHbm": "Maple pool state",
};

type AccountValue = {
  lamports: number;
  data: [string, string];
  owner: string;
  executable: boolean;
  rentEpoch: number;
};

async function fetchFromMainnet(pubkey: string): Promise<AccountValue | null> {
  const res = await fetch(MAINNET_RPC!, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0", id: 1,
      method: "getAccountInfo",
      params: [pubkey, { encoding: "base64" }],
    }),
  });
  const json = (await res.json()) as { result?: { value: AccountValue | null } };
  return json.result?.value ?? null;
}

async function isSurfpoolHealthy(): Promise<boolean> {
  try {
    const res = await fetch(LOCAL_RPC, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "getHealth" }),
    });
    const json = (await res.json()) as { result?: string };
    return json.result === "ok";
  } catch {
    return false;
  }
}

async function injectIntoSurfpool(pubkey: string, account: AccountValue): Promise<void> {
  // surfpool v1.1.1 params: ["pubkey", {lamports, data: hexstring, owner, executable, rentEpoch}]
  const dataHex = Buffer.from(account.data[0], "base64").toString("hex");
  const res = await fetch(LOCAL_RPC, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0", id: 1,
      method: "surfnet_setAccount",
      params: [
        pubkey,
        {
          lamports: account.lamports,
          data: dataHex,
          owner: account.owner,
          executable: account.executable,
          rentEpoch: 0,
        },
      ],
    }),
  });
  const json = (await res.json()) as { error?: unknown };
  if (json.error) {
    console.warn(`    ⚠  surfnet_setAccount ${pubkey}: ${JSON.stringify(json.error)}`);
  }
}

// ─── Root before hook — runs once before all tests in this suite ──────────────

before(async function () {
  if (!MAINNET_RPC) {
    if (FORK_REQUIRED) {
      throw new Error(
        "FORK_REQUIRED=1 but SURFPOOL_DATASOURCE_RPC_URL is not set — " +
          "CI cannot run mainnet-fork tests without a datasource RPC.",
      );
    }
    // No mainnet RPC — individual tests will skip gracefully.
    return;
  }

  this.timeout(120_000);

  // In CI, surfpool itself must be reachable before we try to clone state.
  if (FORK_REQUIRED && !(await isSurfpoolHealthy())) {
    throw new Error(`FORK_REQUIRED=1 but surfpool is not responding at ${LOCAL_RPC}.`);
  }

  console.log(`\n  Cloning ${Object.keys(ACCOUNTS_TO_CLONE).length} mainnet accounts into surfpool…`);

  let ok = 0;
  let fail = 0;

  for (const [pubkey, label] of Object.entries(ACCOUNTS_TO_CLONE)) {
    try {
      const account = await fetchFromMainnet(pubkey);
      if (!account) {
        console.log(`    skip  ${label} (${pubkey.slice(0, 8)}…) — not found on mainnet`);
        fail++;
        continue;
      }
      await injectIntoSurfpool(pubkey, account);
      console.log(`    clone ${label} (${pubkey.slice(0, 8)}…)`);
      ok++;
    } catch (err) {
      console.warn(`    error ${label}: ${err}`);
      fail++;
    }
  }

  console.log(`  Done: ${ok} cloned, ${fail} skipped/failed.\n`);

  if (FORK_REQUIRED && fail > 0) {
    throw new Error(
      `FORK_REQUIRED=1 but ${fail} required mainnet account(s) failed to clone — ` +
        "the fork is incomplete, refusing to run with partial state.",
    );
  }
});
