/**
 * register-adapter.ts
 *
 * CLI for managing adapters in the on-chain Registry.
 *
 * Usage:
 *   npx tsx scripts/register-adapter.ts <command> [options]
 *
 * Commands:
 *   approve    Register a new adapter
 *   revoke     Permanently revoke an adapter (irreversible)
 *   pause      Temporarily pause an adapter
 *   resume     Resume a paused adapter
 *   update     Update adapter metadata
 *   status     Print the RegistryEntry for an adapter
 *   list       List all registered adapters (reads RegistryState.adapter_count)
 *
 * Environment:
 *   ANCHOR_PROVIDER_URL   RPC endpoint (default: http://127.0.0.1:8899)
 *   ANCHOR_WALLET         Path to keypair file (default: ~/.config/solana/id.json)
 *
 * Examples:
 *   npx tsx scripts/register-adapter.ts approve \
 *     --adapter 47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz \
 *     --name "MarginFi USDC" \
 *     --protocol "MarginFi" \
 *     --mint EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
 *
 *   npx tsx scripts/register-adapter.ts status \
 *     --adapter 47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz
 *
 *   npx tsx scripts/register-adapter.ts pause \
 *     --adapter 47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz
 */

import * as anchor from "@coral-xyz/anchor";
import { Program, AnchorProvider, Wallet, BN } from "@coral-xyz/anchor";
import {
  PublicKey,
  Keypair,
  Connection,
  clusterApiUrl,
} from "@solana/web3.js";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";

// ── Program IDs ───────────────────────────────────────────────────────────────

const REGISTRY_ID = new PublicKey("4NP3DgbM7JJDBQEiU9ojUJ3yYnCoEnbBCsACfQz32xdB");

// ── Helpers ───────────────────────────────────────────────────────────────────

function findPDA(seeds: Buffer[], programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, programId)[0];
}

function strToBytes64(s: string): number[] {
  const buf = Buffer.alloc(64, 0);
  Buffer.from(s, "utf-8").copy(buf, 0);
  return Array.from(buf);
}

function bytes64ToString(arr: number[]): string {
  const buf = Buffer.from(arr);
  const end = buf.indexOf(0);
  return buf.slice(0, end === -1 ? 64 : end).toString("utf-8");
}

function loadKeypair(walletPath: string): Keypair {
  const resolved = walletPath.startsWith("~")
    ? path.join(os.homedir(), walletPath.slice(1))
    : walletPath;
  const raw = JSON.parse(fs.readFileSync(resolved, "utf-8"));
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

function loadIdl(name: string): anchor.Idl {
  const idlPath = path.join(
    __dirname,
    "..",
    "target",
    "idl",
    `${name}.json`
  );
  if (!fs.existsSync(idlPath)) {
    throw new Error(
      `IDL not found at ${idlPath}. Run 'anchor build' first.`
    );
  }
  return JSON.parse(fs.readFileSync(idlPath, "utf-8"));
}

function parseArgs(argv: string[]): { command: string; opts: Record<string, string> } {
  const args = argv.slice(2);
  const command = args[0] ?? "help";
  const opts: Record<string, string> = {};
  for (let i = 1; i < args.length; i += 2) {
    const key = args[i].replace(/^--/, "");
    const val = args[i + 1] ?? "";
    opts[key] = val;
  }
  return { command, opts };
}

function requireOpt(opts: Record<string, string>, key: string): string {
  const v = opts[key];
  if (!v) throw new Error(`Missing required option: --${key}`);
  return v;
}

// ── Setup ─────────────────────────────────────────────────────────────────────

async function setup() {
  const rpcUrl =
    process.env.ANCHOR_PROVIDER_URL ?? "http://127.0.0.1:8899";
  const walletPath =
    process.env.ANCHOR_WALLET ??
    path.join(os.homedir(), ".config", "solana", "id.json");

  const connection = new Connection(rpcUrl, "confirmed");
  const keypair = loadKeypair(walletPath);
  const wallet = new Wallet(keypair);
  const provider = new AnchorProvider(connection, wallet, {
    commitment: "confirmed",
  });
  anchor.setProvider(provider);

  const idl = loadIdl("registry");
  const program = new Program(idl, provider) as Program<anchor.Idl>;

  const registryState = findPDA([Buffer.from("registry_state")], REGISTRY_ID);

  console.log(`RPC:      ${rpcUrl}`);
  console.log(`Authority: ${keypair.publicKey.toBase58()}`);
  console.log(`Registry:  ${REGISTRY_ID.toBase58()}`);
  console.log();

  return { program, provider, keypair, registryState };
}

// ── Commands ──────────────────────────────────────────────────────────────────

async function cmdApprove(
  program: Program,
  opts: Record<string, string>,
  authority: Keypair,
  registryState: PublicKey
) {
  const adapterId = new PublicKey(requireOpt(opts, "adapter"));
  const name = requireOpt(opts, "name");
  const protocol = requireOpt(opts, "protocol");
  const mint = new PublicKey(requireOpt(opts, "mint"));

  const registryEntry = findPDA(
    [Buffer.from("registry_entry"), adapterId.toBuffer()],
    REGISTRY_ID
  );

  console.log(`Approving adapter: ${adapterId.toBase58()}`);
  console.log(`  Name:     ${name}`);
  console.log(`  Protocol: ${protocol}`);
  console.log(`  Mint:     ${mint.toBase58()}`);

  const tx = await (program.methods as any)
    .approveAdapter(
      adapterId,
      strToBytes64(name),
      strToBytes64(protocol),
      mint
    )
    .accounts({
      authority: authority.publicKey,
      registryState,
      registryEntry,
      systemProgram: anchor.web3.SystemProgram.programId,
    })
    .signers([authority])
    .rpc();

  console.log(`\nApproved. Tx: ${tx}`);
  console.log(`RegistryEntry: ${registryEntry.toBase58()}`);
}

async function cmdRevoke(
  program: Program,
  opts: Record<string, string>,
  authority: Keypair,
  registryState: PublicKey
) {
  const adapterId = new PublicKey(requireOpt(opts, "adapter"));
  const registryEntry = findPDA(
    [Buffer.from("registry_entry"), adapterId.toBuffer()],
    REGISTRY_ID
  );

  console.log(`Revoking adapter: ${adapterId.toBase58()}`);
  const tx = await (program.methods as any)
    .revokeAdapter(adapterId)
    .accounts({ authority: authority.publicKey, registryState, registryEntry })
    .signers([authority])
    .rpc();
  console.log(`Revoked. Tx: ${tx}`);
}

async function cmdPause(
  program: Program,
  opts: Record<string, string>,
  authority: Keypair,
  registryState: PublicKey
) {
  const adapterId = new PublicKey(requireOpt(opts, "adapter"));
  const registryEntry = findPDA(
    [Buffer.from("registry_entry"), adapterId.toBuffer()],
    REGISTRY_ID
  );

  const tx = await (program.methods as any)
    .pauseAdapter(adapterId)
    .accounts({ authority: authority.publicKey, registryState, registryEntry })
    .signers([authority])
    .rpc();
  console.log(`Paused. Tx: ${tx}`);
}

async function cmdResume(
  program: Program,
  opts: Record<string, string>,
  authority: Keypair,
  registryState: PublicKey
) {
  const adapterId = new PublicKey(requireOpt(opts, "adapter"));
  const registryEntry = findPDA(
    [Buffer.from("registry_entry"), adapterId.toBuffer()],
    REGISTRY_ID
  );

  const tx = await (program.methods as any)
    .resumeAdapter(adapterId)
    .accounts({ authority: authority.publicKey, registryState, registryEntry })
    .signers([authority])
    .rpc();
  console.log(`Resumed. Tx: ${tx}`);
}

async function cmdUpdate(
  program: Program,
  opts: Record<string, string>,
  authority: Keypair,
  registryState: PublicKey
) {
  const adapterId = new PublicKey(requireOpt(opts, "adapter"));
  const registryEntry = findPDA(
    [Buffer.from("registry_entry"), adapterId.toBuffer()],
    REGISTRY_ID
  );

  const name   = opts["name"]     ? strToBytes64(opts["name"])     : null;
  const proto  = opts["protocol"] ? strToBytes64(opts["protocol"]) : null;
  const mint   = opts["mint"]     ? new PublicKey(opts["mint"])     : null;

  const tx = await (program.methods as any)
    .updateAdapter(adapterId, name, proto, mint)
    .accounts({ authority: authority.publicKey, registryState, registryEntry })
    .signers([authority])
    .rpc();
  console.log(`Updated. Tx: ${tx}`);
}

async function cmdStatus(
  program: Program,
  opts: Record<string, string>
) {
  const adapterId = new PublicKey(requireOpt(opts, "adapter"));
  const registryEntry = findPDA(
    [Buffer.from("registry_entry"), adapterId.toBuffer()],
    REGISTRY_ID
  );

  const entry = await (program.account as any).registryEntry.fetch(registryEntry);
  console.log("RegistryEntry:");
  console.log(`  Address:   ${registryEntry.toBase58()}`);
  console.log(`  Adapter:   ${entry.adapterProgramId.toBase58()}`);
  console.log(`  Name:      ${bytes64ToString(entry.name)}`);
  console.log(`  Protocol:  ${bytes64ToString(entry.protocol)}`);
  console.log(`  Mint:      ${entry.assetMint.toBase58()}`);
  console.log(`  Version:   ${entry.version}`);
  console.log(`  Active:    ${entry.isActive}`);
  console.log(`  Paused:    ${entry.isPaused}`);
  console.log(`  Approved:  ${new Date(entry.approvedAt.toNumber() * 1000).toISOString()}`);
  console.log(`  By:        ${entry.approvedBy.toBase58()}`);
}

async function cmdInit(
  program: Program,
  opts: Record<string, string>,
  authority: Keypair,
  registryState: PublicKey
) {
  const authorityPubkey = opts["authority"]
    ? new PublicKey(opts["authority"])
    : authority.publicKey;

  console.log(`Initializing Registry...`);
  console.log(`  Authority: ${authorityPubkey.toBase58()}`);
  console.log(`  State PDA: ${registryState.toBase58()}`);

  const tx = await (program.methods as any)
    .initializeRegistry(authorityPubkey)
    .accounts({
      payer: authority.publicKey,
      registryState,
      systemProgram: anchor.web3.SystemProgram.programId,
    })
    .signers([authority])
    .rpc();

  console.log(`\nRegistry initialized. Tx: ${tx}`);
}

async function cmdList(program: Program, registryState: PublicKey) {
  const state = await (program.account as any).registryState.fetch(registryState);
  console.log(`Registry authority: ${state.authority.toBase58()}`);
  console.log();

  const entries = await (program.account as any).registryEntry.all();
  if (entries.length === 0) {
    console.log("  (no adapters registered)");
    return;
  }
  for (const { publicKey, account } of entries) {
    const active = account.isActive ? "active" : "REVOKED";
    const paused = account.isPaused ? " (PAUSED)" : "";
    console.log(
      `  ${bytes64ToString(account.name).padEnd(30)} ` +
      `${account.adapterProgramId.toBase58().slice(0, 16)}… ` +
      `${active}${paused}`
    );
  }
}

// ── Main ──────────────────────────────────────────────────────────────────────

function printHelp() {
  console.log(`
Solana Yield Adapter Standard — Registry CLI

Usage: npx tsx scripts/register-adapter.ts <command> [options]

Commands:
  init      Initialize the Registry (one-time setup) [--authority <pubkey>]
  approve   --adapter <ID> --name <str> --protocol <str> --mint <pubkey>
  revoke    --adapter <ID>
  pause     --adapter <ID>
  resume    --adapter <ID>
  update    --adapter <ID> [--name <str>] [--protocol <str>] [--mint <pubkey>]
  status    --adapter <ID>
  list

Environment:
  ANCHOR_PROVIDER_URL   RPC endpoint (default: http://127.0.0.1:8899)
  ANCHOR_WALLET         Keypair path (default: ~/.config/solana/id.json)
`);
}

async function main() {
  const { command, opts } = parseArgs(process.argv);

  if (command === "help" || command === "--help" || command === "-h") {
    printHelp();
    return;
  }

  const { program, keypair, registryState } = await setup();

  switch (command) {
    case "init":    await cmdInit(program, opts, keypair, registryState);    break;
    case "approve": await cmdApprove(program, opts, keypair, registryState); break;
    case "revoke":  await cmdRevoke(program, opts, keypair, registryState);  break;
    case "pause":   await cmdPause(program, opts, keypair, registryState);   break;
    case "resume":  await cmdResume(program, opts, keypair, registryState);  break;
    case "update":  await cmdUpdate(program, opts, keypair, registryState);  break;
    case "status":  await cmdStatus(program, opts);                          break;
    case "list":    await cmdList(program, registryState);                   break;
    default:
      console.error(`Unknown command: ${command}`);
      printHelp();
      process.exit(1);
  }
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
});
