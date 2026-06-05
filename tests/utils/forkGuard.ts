import type { Context } from "mocha";

/**
 * When `FORK_REQUIRED=1` (set by CI), the absence of mainnet-fork state is a
 * hard failure instead of a silent skip. Locally the variable is unset, so
 * developers without a datasource RPC still get a graceful skip.
 */
export const FORK_REQUIRED = process.env.FORK_REQUIRED === "1";

const HINT =
  "Run with: SURFPOOL_DATASOURCE_RPC_URL=<mainnet-rpc-url> anchor test --skip-build";

/**
 * Call from a `before` hook when the mainnet fork is unavailable.
 *
 *  - CI (`FORK_REQUIRED=1`): throws, failing the suite so a misconfigured fork
 *    never passes green with every test skipped.
 *  - Local (unset): logs a hint and skips the suite gracefully.
 */
export function skipOrFail(ctx: Context, reason: string): void {
  if (FORK_REQUIRED) {
    throw new Error(`FORK_REQUIRED=1 but mainnet fork is unavailable: ${reason}`);
  }
  console.log(`    ⚠  ${reason} — mainnet fork required. Skipping.`);
  console.log(`       ${HINT}`);
  ctx.skip();
}
