# Program keypairs

These are the **program-ID keypairs** for the workspace programs — the keys that
define each program's on-chain address (the IDs in `Anchor.toml`, `declare_id!`,
and the README). They are committed so that builds and mainnet-fork tests are
fully reproducible: the fork tests in `tests/fork/` invoke the adapters at these
exact addresses, and surfpool deploys each `.so` at its keypair's pubkey.

`target/` is gitignored, so on a fresh checkout (e.g. CI) the keypairs are
absent and `anchor build` would generate random ones — making surfpool deploy
the adapters at the wrong addresses. The CI workflow copies these into
`target/deploy/` before building to pin the canonical IDs.

## Is this safe?

Yes. A program-ID keypair only authorizes the **initial** deploy of a program at
its address. It is **not** the upgrade authority — upgrades to the live devnet
programs are controlled by a separate wallet key that is *not* in this repo.
Publishing these keypairs does not let anyone modify or take over the deployed
programs.

| Keypair | Program ID |
|---|---|
| `dispatcher-keypair.json` | `F6QyZM6rb5i1bDsW9gQMrPBVzZeEfSRbR2JDzDQTJuQ1` |
| `registry-keypair.json` | `4NP3DgbM7JJDBQEiU9ojUJ3yYnCoEnbBCsACfQz32xdB` |
| `marginfi_adapter-keypair.json` | `47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz` |
| `kamino_adapter-keypair.json` | `5ksJ5dU6jAoZaUnpcXtGN69xXewcRcGLTBisQHSkwc44` |
| `drift_adapter-keypair.json` | `BYT5wbAodWevNJRLnaU2Qe87prHWqycBoZh3oWnCXeY8` |
| `jupiter_lp_adapter-keypair.json` | `7JVMN1WEVmXGFdAu5AQsGFfxEAjoL2uD79hEzeo9115E` |
| `maple_adapter-keypair.json` | `EuffaJ2ccu1PnppDd5rTBxPvFXA4u8YQKDj6DyqsyVot` |
| `template_adapter-keypair.json` | `6TiEx46An5whVbruUqyMxYJmCmkUJrGPrtBtuM6N7NyR` |
