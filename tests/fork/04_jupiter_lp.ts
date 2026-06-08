/**
 * Jupiter Perpetuals LP (JLP) Adapter — mainnet-fork integration test
 *
 * Prerequisites:
 *   surfpool start --network mainnet --no-tui --daemon
 *
 * Then run:
 *   npm run test:fork -- --grep "Jupiter LP"
 *
 * Note: The USDC custody oracle pubkey is read dynamically from the custody
 * account at offset 106 (disc + pool + mint + token_account + decimals + is_stable).
 * It is passed as remaining_accounts[0] for deposit and withdraw.
 */

import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  createAssociatedTokenAccountInstruction,
  getAccount,
} from "@solana/spl-token";
import { expect } from "chai";
import { skipOrFail } from "../utils/forkGuard";
import * as fs from "fs";
import * as path from "path";

// ─── surfpool helper (inlined) ────────────────────────────────────────────────

function _encodeTokenAccount(mint: PublicKey, owner: PublicKey, amount: bigint): Buffer {
  const buf = Buffer.alloc(165);
  mint.toBuffer().copy(buf, 0);
  owner.toBuffer().copy(buf, 32);
  buf.writeBigUInt64LE(amount, 64);
  buf.writeUInt32LE(0, 72);
  buf.writeUInt8(1, 108);
  buf.writeUInt32LE(0, 109);
  buf.writeBigUInt64LE(0n, 121);
  buf.writeUInt32LE(0, 129);
  return buf;
}

async function fundTokenAccount(
  rpcUrl: string,
  tokenAccount: PublicKey,
  mint: PublicKey,
  owner: PublicKey,
  amount: bigint
): Promise<void> {
  const data = _encodeTokenAccount(mint, owner, amount);
  const body = {
    jsonrpc: "2.0", id: 1,
    method: "surfnet_setAccount",
    params: [
      tokenAccount.toBase58(),
      {
        lamports: 2_039_280,
        data: data.toString("hex"),
        owner: TOKEN_PROGRAM_ID.toBase58(),
        executable: false,
        rentEpoch: 0,
      },
    ],
  };
  const res = await fetch(rpcUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const json = (await res.json()) as { error?: unknown };
  if (json.error) throw new Error(`surfnet_setAccount: ${JSON.stringify(json.error)}`);
}

// Re-clone accounts from mainnet into surfpool so their data (e.g. oracle publish
// slot) is fresh relative to surfpool's advancing clock — Jupiter rejects stale
// oracle prices. Call right before an instruction that reads the oracles.
// Doves price account stores its publish unix timestamp as i64 at offset 177.
// Jupiter rejects prices older than ~5s, but a cloned price freezes while
// surfpool's clock keeps advancing, so we forge the timestamp slightly ahead of
// surfpool's clock to keep the cloned price "fresh" through the tx.
const DOVES_TS_OFFSET = 177;

async function rpcCall(url: string, method: string, params: any[]): Promise<any> {
  const res = await fetch(url, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  return ((await res.json()) as any).result;
}

async function refreshFromMainnet(localRpc: string, pubkeys: PublicKey[]): Promise<void> {
  // surfpool Clock unix_timestamp (i64 at offset 32 of the Clock sysvar).
  const clk = await rpcCall(localRpc, "getAccountInfo",
    ["SysvarC1ock11111111111111111111111111111111", { encoding: "base64" }]);
  const clkBuf = Buffer.from(clk.value.data[0], "base64");
  const freshTs = clkBuf.readBigInt64LE(32) + 60n;

  const mainnet = process.env.SURFPOOL_DATASOURCE_RPC_URL;
  // Fetch from mainnet if available; otherwise use local clone (data is still correct,
  // only the publish timestamp is stale — we patch it to clock+60 either way).
  const source = mainnet ?? localRpc;
  const fetched = await Promise.all(pubkeys.map(async (pk) =>
    ({ pk, v: (await rpcCall(source, "getAccountInfo", [pk.toBase58(), { encoding: "base64" }]))?.value })));
  await Promise.all(fetched.map(async ({ pk, v }) => {
    if (!v) return;
    const buf = Buffer.from(v.data[0], "base64");
    if (buf.length >= DOVES_TS_OFFSET + 8) buf.writeBigInt64LE(freshTs, DOVES_TS_OFFSET);
    await rpcCall(localRpc, "surfnet_setAccount", [pk.toBase58(),
      { lamports: v.lamports, data: buf.toString("hex"), owner: v.owner, executable: v.executable, rentEpoch: 0 }]);
  }));
}

// ─── Constants ────────────────────────────────────────────────────────────────

const PERP_PROGRAM_ID    = new PublicKey("PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu");
const JLP_POOL           = new PublicKey("5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq");
const JLP_MINT           = new PublicKey("27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4");
const USDC_MINT          = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const USDC_CUSTODY       = new PublicKey("G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa");
const ADAPTER_PROGRAM_ID = new PublicKey("7JVMN1WEVmXGFdAu5AQsGFfxEAjoL2uD79hEzeo9115E");

const DEPOSIT_USDC = 10_000_000; // 10 USDC (6 decimals)

// Jupiter Custody.oracle holds two price feeds used by addLiquidity2/removeLiquidity2:
//   [320..352] doves price account
//   [384..416] pythnet price account
const CUSTODY_DOVES_OFFSET   = 320;
const CUSTODY_PYTHNET_OFFSET = 384;

// Jupiter Perps event-CPI authority PDA.
const EVENT_AUTHORITY = findPDAStatic([Buffer.from("__event_authority")], PERP_PROGRAM_ID);

// AUM remaining accounts (verified against a real addLiquidity2 tx): all pool
// custodies first, then each custody's pythnet price account — in pool.custodies
// order. Jupiter reads these to compute assets-under-management.
const POOL_CUSTODIES: PublicKey[] = [
  "7xS2gz2bTp3fwCC7knJvUWTEU9Tycczu6VhJYKgi1wdz",
  "AQCGyheWPLeo6Qp9WpYS9m3Qj479t7R636N9ey1rEjEn",
  "5Pv3gM9JrFFH883SWAhvJC9RPYmo8UNxuFtv5bMMALkm",
  "G18jKKXQwBbrHeiK3C9MRXhkHsLHf7XgCSisykV46EZa",
  "4vkNeXiYEUizLdrpdPS1eC2mccyM4NUPRtERrk6ZETkk",
].map((s) => new PublicKey(s));
const POOL_PYTHNET_PRICES: PublicKey[] = [
  "FYq2BWQ1V5P1WFBqr3qB2Kb5yHVvSv7upzKodgQE5zXh",
  "AFZnHPzy4mvVCffrVwhewHbFc93uTHvDSFrVH7GtfXF1",
  "hUqAT1KQ7eW1i6Csp9CXYtpPfSAvi835V7wKi5fRfmC",
  "6Jp2xZUTWdDD2ZyUPRzeMdc6AFQ5K3pFgZxk2EijfjnM",
  "Fgc93D641F8N2d1xLjQ4jmShuD3GE3BsCXA56KBQbF5u",
].map((s) => new PublicKey(s));
const AUM_ACCOUNTS: PublicKey[] = [...POOL_CUSTODIES, ...POOL_PYTHNET_PRICES];

// Named custodyDoves/custodyPythnet slots both take the deposit custody's pythnet
// price account (per the real tx); remaining[2..] is the AUM list.
function jupRemaining(custodyPythnet: PublicKey) {
  return [custodyPythnet, custodyPythnet, ...AUM_ACCOUNTS].map((pubkey) => ({
    pubkey, isSigner: false, isWritable: false,
  }));
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function findPDA(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}
function findPDAStatic(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}

// Read the doves + pythnet price accounts from USDC custody data.
// Returns null if the custody is not available (mainnet fork not running).
async function readCustodyOracles(
  conn: Connection
): Promise<{ doves: PublicKey; pythnet: PublicKey } | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const info = await conn.getAccountInfo(USDC_CUSTODY);
    if (info && info.data.length >= CUSTODY_PYTHNET_OFFSET + 32) {
      const doves = new PublicKey(info.data.slice(CUSTODY_DOVES_OFFSET, CUSTODY_DOVES_OFFSET + 32));
      const pythnet = new PublicKey(info.data.slice(CUSTODY_PYTHNET_OFFSET, CUSTODY_PYTHNET_OFFSET + 32));
      if (doves.equals(PublicKey.default) || pythnet.equals(PublicKey.default)) return null;
      return { doves, pythnet };
    }
    await new Promise(r => setTimeout(r, 2000));
  }
  return null;
}

// ─── Suite ────────────────────────────────────────────────────────────────────

describe("Jupiter LP Adapter", () => {
  const RPC_URL = "http://127.0.0.1:8899";
  const connection = new Connection(RPC_URL, "confirmed");
  const owner = Keypair.generate();

  // Adapter PDAs
  const adapterPosition = findPDA(
    [Buffer.from("jlp_pos"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );
  const jlpAuthority = findPDA(
    [Buffer.from("jlp_auth"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );

  // Jupiter Perpetuals PDAs
  const transferAuthority = findPDA(
    [Buffer.from("transfer_authority")],
    PERP_PROGRAM_ID
  );
  const perpetuals = findPDA(
    [Buffer.from("perpetuals")],
    PERP_PROGRAM_ID
  );
  const custodyTokenAccount = findPDA(
    [Buffer.from("custody_token_account"), JLP_POOL.toBuffer(), USDC_MINT.toBuffer()],
    PERP_PROGRAM_ID
  );

  // Token accounts
  const authorityJlpAta  = getAssociatedTokenAddressSync(JLP_MINT, jlpAuthority, true);
  const authorityUsdcAta = getAssociatedTokenAddressSync(USDC_MINT, jlpAuthority, true);
  const userUsdcAta      = getAssociatedTokenAddressSync(USDC_MINT, owner.publicKey);

  let program: Program;
  let dovesPrice: PublicKey;
  let pythnetPrice: PublicKey;

  before(async function () {
    this.timeout(60_000);

    const sig = await connection.requestAirdrop(owner.publicKey, 10 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig);

    // Read doves + pythnet price feeds from USDC custody — validates fork is up
    const oracles = await readCustodyOracles(connection);
    if (!oracles) {
      skipOrFail(this, "Jupiter USDC custody not available");
      return;
    }
    dovesPrice = oracles.doves;
    pythnetPrice = oracles.pythnet;

    const idlPath = path.join(__dirname, "../../target/idl/jupiter_lp_adapter.json");
    const idl = JSON.parse(fs.readFileSync(idlPath, "utf-8"));
    const provider = new anchor.AnchorProvider(
      connection,
      new anchor.Wallet(owner),
      { commitment: "confirmed", skipPreflight: false }
    );
    program = new Program(idl, provider);

    // Create user's USDC ATA and fund it (source for deposit)
    const createAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        owner.publicKey,
        userUsdcAta,
        owner.publicKey,
        USDC_MINT
      )
    );
    await sendAndConfirmTransaction(connection, createAta, [owner]);
    await fundTokenAccount(RPC_URL, userUsdcAta, USDC_MINT, owner.publicKey, BigInt(DEPOSIT_USDC * 10));
  });

  // ── initialize_position ────────────────────────────────────────────────────

  it("initialize_position creates adapter_position and authority ATAs", async () => {
    await program.methods
      .initializePosition()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        jlpAuthority,
        authorityJlpAta,
        authorityUsdcAta,
        jlpMint: JLP_MINT,
        usdcMint: USDC_MINT,
        perpProgram: PERP_PROGRAM_ID,
        usdcCustody: USDC_CUSTODY,
        pool: JLP_POOL,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).jupiterLpAdapterPosition.fetch(adapterPosition);
    expect(pos.owner.toBase58()).to.equal(owner.publicKey.toBase58());
    expect(pos.shares.toNumber()).to.equal(0);

    // Both ATAs should exist now
    await getAccount(connection, authorityJlpAta);
    await getAccount(connection, authorityUsdcAta);
  });

  // ── deposit ────────────────────────────────────────────────────────────────

  it("deposit(10 USDC) mints JLP tokens and increases shares", async () => {
    const jlpMintInfoBefore = await connection.getAccountInfo(JLP_MINT);
    const supplyBefore = jlpMintInfoBefore
      ? Number(BigInt("0x" + jlpMintInfoBefore.data.slice(36, 44).reverse().reduce((s, b) => s + b.toString(16).padStart(2, "0"), "")))
      : 0;

    // Keep oracle prices fresh vs surfpool's advancing clock.
    await refreshFromMainnet(RPC_URL, POOL_PYTHNET_PRICES);

    await program.methods
      .deposit(new BN(DEPOSIT_USDC))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        jlpAuthority,
        perpProgram: PERP_PROGRAM_ID,
        pool: JLP_POOL,
        custody: USDC_CUSTODY,
        custodyTokenAccount,
        transferAuthority,
        perpetuals,
        jlpMint: JLP_MINT,
        authorityJlpAta,
        authorityUsdcAta,
        userUsdcAta,
        usdcMintAccount: USDC_MINT,
        eventAuthority: EVENT_AUTHORITY,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .remainingAccounts(jupRemaining(pythnetPrice))
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
      ])
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).jupiterLpAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.be.greaterThan(0);

    const jlpAta = await getAccount(connection, authorityJlpAta);
    expect(Number(jlpAta.amount)).to.equal(pos.shares.toNumber());
  });

  // ── current_value ──────────────────────────────────────────────────────────

  it("current_value returns USDC value ≈ deposited amount", async () => {
    // Anchor .simulate() lacks returnData in v0.31; use connection.simulateTransaction directly.
    const tx = await program.methods
      .currentValue()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        pool: JLP_POOL,
        jlpMint: JLP_MINT,
      })
      .transaction();

    tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
    tx.feePayer = owner.publicKey;

    const result = await connection.simulateTransaction(tx, [owner]);
    expect(result.value.err).to.be.null;
    expect(result.value.returnData).to.not.be.null;

    const raw = Buffer.from(result.value.returnData!.data[0], "base64");
    const value = Number(raw.readBigUInt64LE(0));

    expect(value).to.be.greaterThan(0);
    // JLP price ≥ $1 (pool AUM / total supply), allow ±10% rounding
    expect(value).to.be.approximately(DEPOSIT_USDC, DEPOSIT_USDC * 0.1);
  });

  // ── withdraw all ───────────────────────────────────────────────────────────

  it("withdraw(0) burns all JLP and returns USDC to user", async () => {
    const userAtaBefore = await getAccount(connection, userUsdcAta);

    // Keep oracle prices fresh vs surfpool's advancing clock.
    await refreshFromMainnet(RPC_URL, POOL_PYTHNET_PRICES);

    await program.methods
      .withdraw(new BN(0)) // 0 = withdraw all
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        jlpAuthority,
        perpProgram: PERP_PROGRAM_ID,
        pool: JLP_POOL,
        custody: USDC_CUSTODY,
        custodyTokenAccount,
        transferAuthority,
        perpetuals,
        jlpMint: JLP_MINT,
        authorityJlpAta,
        usdcMintAccount: USDC_MINT,
        authorityUsdcAta,
        userUsdcAta,
        eventAuthority: EVENT_AUTHORITY,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .remainingAccounts(jupRemaining(pythnetPrice))
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
      ])
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).jupiterLpAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.equal(0);

    const jlpAta = await getAccount(connection, authorityJlpAta);
    expect(Number(jlpAta.amount)).to.equal(0);

    const userAtaAfter = await getAccount(connection, userUsdcAta);
    // Should receive back close to deposited amount (JLP price ≥ $1)
    expect(Number(userAtaAfter.amount)).to.be.greaterThan(Number(userAtaBefore.amount));
    expect(Number(userAtaAfter.amount)).to.be.greaterThanOrEqual(DEPOSIT_USDC * 0.9);
  });
});
