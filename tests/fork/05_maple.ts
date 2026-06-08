/**
 * Maple (syrupUSDC) Adapter — mainnet-fork integration test
 *
 * Prerequisites:
 *   surfpool start --network mainnet --no-tui --daemon   (with a real datasource RPC)
 *
 * Then:
 *   npm run test:fork -- --grep "Maple"
 *
 * Maple has NO native deposit program on Solana — syrupUSDC is a Chainlink-CCIP
 * bridge token. To offer the uniform USDC-in / USDC-out interface, the adapter
 * routes through the live Orca whirlpool (syrupUSDC/USDC):
 *   deposit(USDC)  -> swap USDC -> syrupUSDC, custody it
 *   withdraw(shares) -> swap syrupUSDC -> USDC, pay the user
 *   current_value  -> price custodied syrupUSDC against the whirlpool
 *
 * The whirlpool, its vaults, oracle and tick arrays are lazily cloned from mainnet
 * by surfpool on first read; we only inject the user's USDC.
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

// ─── Constants (verified on mainnet) ────────────────────────────────────────────

const SYRUP_USDC_MINT  = new PublicKey("AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj");
const USDC_MINT        = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const WHIRLPOOL_PROGRAM = new PublicKey("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
const SYRUP_WHIRLPOOL  = new PublicKey("6fteKNvMdv7tYmBoJHhj1jx6rHcEwC6RdSEmVpyS613J");
const WHIRLPOOL_VAULT_A = new PublicKey("FM2RuqFYo9umA1yc5FyQn6pSDZJZ1MXAdaekJZ4dQCvi"); // syrupUSDC
const WHIRLPOOL_VAULT_B = new PublicKey("Fw6Xr45rBBrXbWJd5ZbSg44kacrKRLef4rHkZ8gWC5Ab"); // USDC
const WHIRLPOOL_ORACLE = new PublicKey("H7j5FQpwTUMwxrWeuyrLr5Z9oHsPFiaRqNaERVsuE1c8");
const MEMO_PROGRAM     = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
const ADAPTER_PROGRAM_ID = new PublicKey("EuffaJ2ccu1PnppDd5rTBxPvFXA4u8YQKDj6DyqsyVot");

const DEPOSIT_USDC = 50_000_000; // 50 USDC (6 decimals)
const TICKS_PER_ARRAY = 88;

// ─── surfpool helper: inject an SPL token account ───────────────────────────────

function encodeTokenAccount(mint: PublicKey, owner: PublicKey, amount: bigint): Buffer {
  const buf = Buffer.alloc(165);
  mint.toBuffer().copy(buf, 0);
  owner.toBuffer().copy(buf, 32);
  buf.writeBigUInt64LE(amount, 64);
  buf.writeUInt8(1, 108); // state = initialized
  return buf;
}

async function fundTokenAccount(
  rpcUrl: string,
  tokenAccount: PublicKey,
  mint: PublicKey,
  owner: PublicKey,
  amount: bigint
): Promise<void> {
  const data = encodeTokenAccount(mint, owner, amount);
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

// ─── Whirlpool / tick-array helpers ─────────────────────────────────────────────

function findPDA(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}

/// Orca tick-array PDA: [b"tick_array", whirlpool, start_tick_index_as_ascii].
function tickArrayPda(whirlpool: PublicKey, startTick: number): PublicKey {
  return findPDA(
    [Buffer.from("tick_array"), whirlpool.toBuffer(), Buffer.from(startTick.toString())],
    WHIRLPOOL_PROGRAM
  );
}

/// Start tick index of the array containing `tick`, shifted by `offset` arrays.
function startTickIndex(tick: number, tickSpacing: number, offset: number): number {
  const span = tickSpacing * TICKS_PER_ARRAY;
  return Math.floor(tick / span) * span + offset * span;
}

/// The three tick arrays a swap needs, in the order of traversal.
/// a_to_b (price falls) walks to lower starts; b_to_a (price rises) walks higher.
function swapTickArrays(
  whirlpool: PublicKey,
  tickCurrent: number,
  tickSpacing: number,
  aToB: boolean
): [PublicKey, PublicKey, PublicKey] {
  const dir = aToB ? -1 : 1;
  return [
    tickArrayPda(whirlpool, startTickIndex(tickCurrent, tickSpacing, 0)),
    tickArrayPda(whirlpool, startTickIndex(tickCurrent, tickSpacing, dir)),
    tickArrayPda(whirlpool, startTickIndex(tickCurrent, tickSpacing, dir * 2)),
  ];
}

async function readWhirlpool(conn: Connection): Promise<{ tickSpacing: number; tickCurrent: number } | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const info = await conn.getAccountInfo(SYRUP_WHIRLPOOL);
    if (info && info.data.length >= 85) {
      const tickSpacing = info.data.readUInt16LE(41);
      const tickCurrent = info.data.readInt32LE(81);
      return { tickSpacing, tickCurrent };
    }
    await new Promise(r => setTimeout(r, 2000));
  }
  return null;
}

const cb = () => ComputeBudgetProgram.setComputeUnitLimit({ units: 600_000 });

// ─── Suite ──────────────────────────────────────────────────────────────────────

describe("Maple Adapter", () => {
  const RPC_URL = "http://127.0.0.1:8899";
  const connection = new Connection(RPC_URL, "confirmed");
  const owner = Keypair.generate();

  const adapterPosition = findPDA(
    [Buffer.from("maple_pos"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );
  const mapleAuthority = findPDA(
    [Buffer.from("maple_auth"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );

  const authoritySyrupAta = getAssociatedTokenAddressSync(SYRUP_USDC_MINT, mapleAuthority, true);
  const authorityUsdcAta = getAssociatedTokenAddressSync(USDC_MINT, mapleAuthority, true);
  const userUsdcAta = getAssociatedTokenAddressSync(USDC_MINT, owner.publicKey);

  let program: Program;
  let pool: { tickSpacing: number; tickCurrent: number };

  before(async function () {
    this.timeout(90_000);

    const sig = await connection.requestAirdrop(owner.publicKey, 10 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig);

    // Whirlpool must be reachable (proves the fork has a live mainnet datasource).
    const wp = await readWhirlpool(connection);
    if (!wp) {
      skipOrFail(this, "syrupUSDC/USDC whirlpool not available on fork");
      return;
    }
    pool = wp;

    const idlPath = path.join(__dirname, "../../target/idl/maple_adapter.json");
    const idl = JSON.parse(fs.readFileSync(idlPath, "utf-8"));
    const provider = new anchor.AnchorProvider(
      connection,
      new anchor.Wallet(owner),
      { commitment: "confirmed", skipPreflight: false }
    );
    program = new Program(idl, provider);

    // Create + fund the user's USDC ATA (the deposit source).
    const createAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(owner.publicKey, userUsdcAta, owner.publicKey, USDC_MINT)
    );
    await sendAndConfirmTransaction(connection, createAta, [owner]);
    await fundTokenAccount(RPC_URL, userUsdcAta, USDC_MINT, owner.publicKey, BigInt(DEPOSIT_USDC * 2));
  });

  // ── initialize_position ──────────────────────────────────────────────────────

  it("initialize_position creates the position and both custody ATAs", async () => {
    await program.methods
      .initializePosition()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        mapleAuthority,
        authoritySyrupAta,
        authorityUsdcAta,
        syrupUsdcMint: SYRUP_USDC_MINT,
        usdcMint: USDC_MINT,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).mapleAdapterPosition.fetch(adapterPosition);
    expect(pos.owner.toBase58()).to.equal(owner.publicKey.toBase58());
    expect(pos.shares.toNumber()).to.equal(0);

    const syrupAta = await getAccount(connection, authoritySyrupAta);
    expect(syrupAta.mint.toBase58()).to.equal(SYRUP_USDC_MINT.toBase58());
    const usdcAta = await getAccount(connection, authorityUsdcAta);
    expect(usdcAta.mint.toBase58()).to.equal(USDC_MINT.toBase58());
  });

  // ── deposit (USDC -> swap -> syrupUSDC) ────────────────────────────────────────

  it("deposit swaps USDC into syrupUSDC and records shares", async () => {
    // b_to_a (USDC -> syrupUSDC): a_to_b = false.
    const [t0, t1, t2] = swapTickArrays(SYRUP_WHIRLPOOL, pool.tickCurrent, pool.tickSpacing, false);

    await program.methods
      .deposit(new BN(DEPOSIT_USDC))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        mapleAuthority,
        userUsdcAta,
        authorityUsdcAta,
        authoritySyrupAta,
        whirlpool: SYRUP_WHIRLPOOL,
        tokenMintA: SYRUP_USDC_MINT,
        tokenMintB: USDC_MINT,
        tokenVaultA: WHIRLPOOL_VAULT_A,
        tokenVaultB: WHIRLPOOL_VAULT_B,
        tickArray0: t0,
        tickArray1: t1,
        tickArray2: t2,
        oracle: WHIRLPOOL_ORACLE,
        whirlpoolProgram: WHIRLPOOL_PROGRAM,
        tokenProgram: TOKEN_PROGRAM_ID,
        memoProgram: MEMO_PROGRAM,
      })
      .preInstructions([cb()])
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).mapleAdapterPosition.fetch(adapterPosition);
    // syrupUSDC > 1 USDC each, so for 50 USDC we receive < 50e6 syrupUSDC but > 0.
    expect(pos.shares.toNumber()).to.be.greaterThan(0);
    expect(pos.shares.toNumber()).to.be.lessThan(DEPOSIT_USDC);

    const custody = await getAccount(connection, authoritySyrupAta);
    expect(Number(custody.amount)).to.equal(pos.shares.toNumber());
  });

  // ── current_value ──────────────────────────────────────────────────────────────

  it("current_value prices the position in USDC (≈ deposit, minus swap fee)", async () => {
    const tx = await program.methods
      .currentValue()
      .accounts({ owner: owner.publicKey, adapterPosition, whirlpool: SYRUP_WHIRLPOOL })
      .transaction();
    tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
    tx.feePayer = owner.publicKey;

    const result = await connection.simulateTransaction(tx, [owner]);
    expect(result.value.err).to.be.null;

    const raw = Buffer.from(result.value.returnData!.data[0], "base64");
    const value = Number(raw.readBigUInt64LE(0));

    // Value ≈ what we paid, net of the swap fee/spread already taken on entry.
    expect(value).to.be.greaterThan(DEPOSIT_USDC * 0.97);
    expect(value).to.be.lessThanOrEqual(DEPOSIT_USDC);
  });

  // ── withdraw all (syrupUSDC -> swap -> USDC) ────────────────────────────────────

  it("withdraw(0) swaps all syrupUSDC back to USDC and zeroes shares", async () => {
    const userBefore = await getAccount(connection, userUsdcAta);
    // a_to_b (syrupUSDC -> USDC): a_to_b = true.
    const [t0, t1, t2] = swapTickArrays(SYRUP_WHIRLPOOL, pool.tickCurrent, pool.tickSpacing, true);

    await program.methods
      .withdraw(new BN(0)) // 0 = withdraw all
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        mapleAuthority,
        authoritySyrupAta,
        authorityUsdcAta,
        userUsdcAta,
        whirlpool: SYRUP_WHIRLPOOL,
        tokenMintA: SYRUP_USDC_MINT,
        tokenMintB: USDC_MINT,
        tokenVaultA: WHIRLPOOL_VAULT_A,
        tokenVaultB: WHIRLPOOL_VAULT_B,
        tickArray0: t0,
        tickArray1: t1,
        tickArray2: t2,
        oracle: WHIRLPOOL_ORACLE,
        whirlpoolProgram: WHIRLPOOL_PROGRAM,
        tokenProgram: TOKEN_PROGRAM_ID,
        memoProgram: MEMO_PROGRAM,
      })
      .preInstructions([cb()])
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).mapleAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.equal(0);

    const custody = await getAccount(connection, authoritySyrupAta);
    expect(Number(custody.amount)).to.equal(0);

    // User got USDC back (round-trip loses ~2× swap fee + spread; expect ≥ 96%).
    const userAfter = await getAccount(connection, userUsdcAta);
    const received = Number(userAfter.amount) - Number(userBefore.amount);
    expect(received).to.be.greaterThan(DEPOSIT_USDC * 0.96);
  });
});
