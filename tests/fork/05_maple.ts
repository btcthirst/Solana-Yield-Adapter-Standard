/**
 * Maple Finance syrupUSDC Adapter — mainnet-fork integration test
 *
 * Prerequisites:
 *   surfpool start --network mainnet --no-tui --daemon
 *
 * Then run:
 *   npm run test:fork -- --grep "Maple"
 *
 * Unlike other adapters, Maple performs no external protocol CPI —
 * it simply custodies syrupUSDC tokens in an authority-owned ATA.
 * syrupUSDC is injected via surfnet_setAccount for the test.
 *
 * current_value falls back to NAV = 1.0 if pool_state is unavailable,
 * so value ≥ deposited amount regardless of mainnet NAV.
 */

import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import {
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

// ─── Constants ────────────────────────────────────────────────────────────────

const SYRUP_USDC_MINT    = new PublicKey("AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj");
const MAPLE_POOL_STATE   = new PublicKey("HrTBpF3LqSxXnjnYdR4htnBLyMHNZ6eNaDZGPundvHbm");
const ADAPTER_PROGRAM_ID = new PublicKey("EuffaJ2ccu1PnppDd5rTBxPvFXA4u8YQKDj6DyqsyVot");

const DEPOSIT_SYRUP = 10_000_000; // 10 syrupUSDC (6 decimals, same as USDC)

// ─── Helpers ──────────────────────────────────────────────────────────────────

function findPDA(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}

// Validate mainnet fork is up by checking syrupUSDC mint exists.
// The initialize_position instruction requires this account as a typed Mint.
async function checkMintAvailable(conn: Connection): Promise<boolean> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const info = await conn.getAccountInfo(SYRUP_USDC_MINT);
    if (info && info.data.length >= 82) return true; // SPL Mint is 82 bytes
    await new Promise(r => setTimeout(r, 2000));
  }
  return false;
}

// ─── Suite ────────────────────────────────────────────────────────────────────

describe("Maple Adapter", () => {
  const RPC_URL = "http://127.0.0.1:8899";
  const connection = new Connection(RPC_URL, "confirmed");
  const owner = Keypair.generate();

  // Adapter PDAs  [b"maple_pos", owner] and [b"maple_auth", owner]
  const adapterPosition = findPDA(
    [Buffer.from("maple_pos"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );
  const mapleAuthority = findPDA(
    [Buffer.from("maple_auth"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );

  // Token accounts
  // Custody: syrupUSDC ATA owned by mapleAuthority (created in initialize_position)
  const authoritySyrupAta = getAssociatedTokenAddressSync(SYRUP_USDC_MINT, mapleAuthority, true);
  // User's syrupUSDC ATA — source for deposit, destination for withdraw
  const userSyrupAta = getAssociatedTokenAddressSync(SYRUP_USDC_MINT, owner.publicKey);

  let program: Program;

  before(async function () {
    this.timeout(60_000);

    const sig = await connection.requestAirdrop(owner.publicKey, 10 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig);

    // syrupUSDC mint must exist for initialize_position to work
    const mintAvailable = await checkMintAvailable(connection);
    if (!mintAvailable) {
      skipOrFail(this, "syrupUSDC mint not available");
      return;
    }

    const idlPath = path.join(__dirname, "../../target/idl/maple_adapter.json");
    const idl = JSON.parse(fs.readFileSync(idlPath, "utf-8"));
    const provider = new anchor.AnchorProvider(
      connection,
      new anchor.Wallet(owner),
      { commitment: "confirmed", skipPreflight: false }
    );
    program = new Program(idl, provider);

    // Create user's syrupUSDC ATA and fund it via surfnet_setAccount.
    // syrupUSDC is normally bridged from Ethereum or swapped on-chain;
    // here we inject it directly for testing purposes.
    const createAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        owner.publicKey,
        userSyrupAta,
        owner.publicKey,
        SYRUP_USDC_MINT
      )
    );
    await sendAndConfirmTransaction(connection, createAta, [owner]);
    await fundTokenAccount(
      RPC_URL,
      userSyrupAta,
      SYRUP_USDC_MINT,
      owner.publicKey,
      BigInt(DEPOSIT_SYRUP * 10) // 100 syrupUSDC
    );
  });

  // ── initialize_position ────────────────────────────────────────────────────

  it("initialize_position creates adapter_position and authority custody ATA", async () => {
    await program.methods
      .initializePosition()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        mapleAuthority,
        authoritySyrupAta,
        syrupUsdcMint: SYRUP_USDC_MINT,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).mapleAdapterPosition.fetch(adapterPosition);
    expect(pos.owner.toBase58()).to.equal(owner.publicKey.toBase58());
    expect(pos.shares.toNumber()).to.equal(0);

    // Authority custody ATA should exist now
    const ata = await getAccount(connection, authoritySyrupAta);
    expect(ata.mint.toBase58()).to.equal(SYRUP_USDC_MINT.toBase58());
    expect(ata.owner.toBase58()).to.equal(mapleAuthority.toBase58());
  });

  // ── deposit ────────────────────────────────────────────────────────────────

  it("deposit transfers syrupUSDC into custody and records shares", async () => {
    const ataBefore = await getAccount(connection, authoritySyrupAta);

    await program.methods
      .deposit(new BN(DEPOSIT_SYRUP))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        mapleAuthority,
        userSyrupAta,
        authoritySyrupAta,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).mapleAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.equal(DEPOSIT_SYRUP);

    const ataAfter = await getAccount(connection, authoritySyrupAta);
    expect(Number(ataAfter.amount) - Number(ataBefore.amount)).to.equal(DEPOSIT_SYRUP);
  });

  // ── current_value ──────────────────────────────────────────────────────────

  it("current_value returns USDC value ≥ deposited amount (NAV ≥ 1.0)", async () => {
    // Anchor .simulate() lacks returnData in v0.31; use connection.simulateTransaction directly.
    const tx = await program.methods
      .currentValue()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        poolState: MAPLE_POOL_STATE,
      })
      .transaction();

    tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
    tx.feePayer = owner.publicKey;

    const result = await connection.simulateTransaction(tx, [owner]);
    expect(result.value.err).to.be.null;
    expect(result.value.returnData).to.not.be.null;

    const raw = Buffer.from(result.value.returnData!.data[0], "base64");
    const value = Number(raw.readBigUInt64LE(0));

    // NAV is always >= 1.0 (yield-bearing), so value >= shares.
    // Fallback NAV = 1.0 if pool_state unavailable → value == DEPOSIT_SYRUP exactly.
    expect(value).to.be.greaterThanOrEqual(DEPOSIT_SYRUP);
  });

  // ── withdraw all ───────────────────────────────────────────────────────────

  it("withdraw(0) returns all syrupUSDC to user and zeroes shares", async () => {
    const userAtaBefore = await getAccount(connection, userSyrupAta);

    await program.methods
      .withdraw(new BN(0)) // 0 = withdraw all
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        mapleAuthority,
        authoritySyrupAta,
        userSyrupAta,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).mapleAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.equal(0);

    const custodyAta = await getAccount(connection, authoritySyrupAta);
    expect(Number(custodyAta.amount)).to.equal(0);

    const userAtaAfter = await getAccount(connection, userSyrupAta);
    expect(Number(userAtaAfter.amount) - Number(userAtaBefore.amount)).to.equal(DEPOSIT_SYRUP);
  });
});
