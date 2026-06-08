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

/**
 * Skip a suite that is blocked by a *permanent external* condition — i.e. the
 * mainnet fork is healthy, but the underlying protocol cannot be exercised
 * because of an upstream change outside this repo's control.
 *
 * Unlike `skipOrFail`, this ALWAYS skips (never throws), even under
 * `FORK_REQUIRED=1`. The blocker is not a misconfiguration we can fix by
 * supplying a working RPC, so failing CI would be misleading — the skip is the
 * correct, honest outcome. The reason is logged loudly so it is never silent.
 *
 * Current sole use: the Drift adapter. Drift's deployed program
 * `dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH` has had **all instructions
 * commented out** upstream (latest commit on Drift's program repo:
 * "comment out all ixs"), so every CPI returns AnchorError 101
 * (InstructionFallbackNotFound) — byte-identically to a bogus discriminator.
 * No adapter code can pass a real fork test against it. See
 * Docs/troubleshooting/drift-fork-issues.md for the full proof.
 */
export function skipKnownBlocker(ctx: Context, reason: string): void {
  console.log(`    ⛔ KNOWN EXTERNAL BLOCKER — skipping (not a failure): ${reason}`);
  ctx.skip();
}
