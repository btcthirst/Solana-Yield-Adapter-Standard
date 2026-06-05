/**
 * Drift Insurance Fund USDC Adapter — mainnet-fork integration test
 *
 * Prerequisites:
 *   surfpool start --network mainnet --no-tui --daemon
 *
 * Then run:
 *   npm run test:fork -- --grep "Drift"
 *
 * Note: withdraw is a 2-step process with ~13-day cooldown on mainnet.
 * The test bypasses the cooldown by zeroing unstaking_period in the spot_market
 * account via surfnet_setAccount between withdraw step 1 and step 2.
 */

import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import {
  AccountInfo,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  Transaction,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  createAssociatedTokenAccountInstruction,
  getAccount,
} from "@solana/spl-token";
import { expect } from "chai";
import { skipOrFail } from "../utils/forkGuard";
import * as fs from "fs";
import * as path from "path";

// ─── surfpool helpers (inlined) ───────────────────────────────────────────────

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

async function surfnetSetAccount(
  rpcUrl: string,
  pubkey: PublicKey,
  account: {
    lamports: number;
    data: Buffer;
    owner: PublicKey;
    executable: boolean;
  }
): Promise<void> {
  const body = {
    jsonrpc: "2.0", id: 1,
    method: "surfnet_setAccount",
    params: [
      pubkey.toBase58(),
      {
        lamports: account.lamports,
        data: account.data.toString("hex"),
        owner: account.owner.toBase58(),
        executable: account.executable,
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

async function fundTokenAccount(
  rpcUrl: string,
  tokenAccount: PublicKey,
  mint: PublicKey,
  owner: PublicKey,
  amount: bigint
): Promise<void> {
  const data = _encodeTokenAccount(mint, owner, amount);
  await surfnetSetAccount(rpcUrl, tokenAccount, {
    lamports: 2_039_280,
    data,
    owner: TOKEN_PROGRAM_ID,
    executable: false,
  });
}

// ─── Constants ────────────────────────────────────────────────────────────────

const DRIFT_PROGRAM_ID   = new PublicKey("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");
const DRIFT_STATE        = new PublicKey("5zpq7DvB6UdFFvpmBPspGPNfUGoBRRCE2HHg5u3gxcsN");
const DRIFT_SIGNER       = new PublicKey("JCNCMFXo5M5qwUPg2Utu1u6YWp3MbygxqBsBeXXJfrw");
const USDC_SPOT_MARKET   = new PublicKey("6gMq3mRCKf8aP3ttTyYhuijVZ2LGi14oDsBbkgubfLB3");
const USDC_IF_VAULT      = new PublicKey("2CqkQvYxp9Mq4PqLvAQ1eryYxebUh4Liyn5YMDtXsYci");
const USDC_MINT          = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const ADAPTER_PROGRAM_ID = new PublicKey("BYT5wbAodWevNJRLnaU2Qe87prHWqycBoZh3oWnCXeY8");

const DEPOSIT_USDC = 10_000_000; // 10 USDC (6 decimals)

// SpotMarket layout offset for unstaking_period — see programs/drift-adapter/src/cpi.rs
const UNSTAKING_PERIOD_OFFSET = 384;

// ─── Helpers ──────────────────────────────────────────────────────────────────

function findPDA(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}

// Read spot_market to verify mainnet fork is available.
// Returns the raw AccountInfo so callers can inspect / modify it.
async function readSpotMarket(conn: Connection): Promise<AccountInfo<Buffer> | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const info = await conn.getAccountInfo(USDC_SPOT_MARKET);
    if (info && info.data.length > UNSTAKING_PERIOD_OFFSET + 8) {
      return info as AccountInfo<Buffer>;
    }
    await new Promise(r => setTimeout(r, 2000));
  }
  return null;
}

// Override the spot_market's unstaking_period field to 0 so that the
// withdraw step-2 check (now >= withdrawal_request_ts + cooldown) passes immediately.
async function zeroSpotMarketCooldown(
  rpcUrl: string,
  spotMarketInfo: AccountInfo<Buffer>
): Promise<void> {
  const data = Buffer.from(spotMarketInfo.data);
  data.writeBigInt64LE(0n, UNSTAKING_PERIOD_OFFSET);
  await surfnetSetAccount(rpcUrl, USDC_SPOT_MARKET, {
    lamports: spotMarketInfo.lamports,
    data,
    owner: DRIFT_PROGRAM_ID,
    executable: false,
  });
}

// ─── Suite ────────────────────────────────────────────────────────────────────

describe("Drift Adapter", () => {
  const RPC_URL = "http://127.0.0.1:8899";
  const connection = new Connection(RPC_URL, "confirmed");
  const owner = Keypair.generate();

  // Adapter PDA
  const adapterPosition = findPDA(
    [Buffer.from("drift_pos"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );

  // Drift-program PDAs
  const userStats = findPDA(
    [Buffer.from("user_stats"), owner.publicKey.toBuffer()],
    DRIFT_PROGRAM_ID
  );
  // market_index = 0 → LE bytes = [0, 0]
  const insuranceFundStake = findPDA(
    [Buffer.from("insurance_fund_stake"), owner.publicKey.toBuffer(), Buffer.from([0, 0])],
    DRIFT_PROGRAM_ID
  );
  const spotMarketVault = findPDA(
    [Buffer.from("spot_market_vault"), Buffer.from([0, 0])],
    DRIFT_PROGRAM_ID
  );

  // User's USDC ATA — source for deposit, destination for withdraw
  const userTokenAccount = getAssociatedTokenAddressSync(USDC_MINT, owner.publicKey);

  let program: Program;
  let spotMarketInfo: AccountInfo<Buffer>;

  before(async function () {
    this.timeout(60_000);

    const sig = await connection.requestAirdrop(owner.publicKey, 10 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig);

    // Validate mainnet fork is up
    const info = await readSpotMarket(connection);
    if (!info) {
      skipOrFail(this, "Drift spot market not available");
      return;
    }
    spotMarketInfo = info;

    const idlPath = path.join(__dirname, "../../target/idl/drift_adapter.json");
    const idl = JSON.parse(fs.readFileSync(idlPath, "utf-8"));
    const provider = new anchor.AnchorProvider(
      connection,
      new anchor.Wallet(owner),
      { commitment: "confirmed", skipPreflight: false }
    );
    program = new Program(idl, provider);

    // Create owner's USDC ATA and fund it for deposit
    const createAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        owner.publicKey,
        userTokenAccount,
        owner.publicKey,
        USDC_MINT
      )
    );
    await sendAndConfirmTransaction(connection, createAta, [owner]);
    await fundTokenAccount(RPC_URL, userTokenAccount, USDC_MINT, owner.publicKey, BigInt(DEPOSIT_USDC * 10));
  });

  // ── initialize_position ────────────────────────────────────────────────────

  it("initialize_position creates adapter position, user_stats, and IF stake account", async () => {
    await program.methods
      .initializePosition()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        userStats,
        insuranceFundStake,
        state: DRIFT_STATE,
        spotMarket: USDC_SPOT_MARKET,
        driftProgram: DRIFT_PROGRAM_ID,
        rent: SYSVAR_RENT_PUBKEY,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).driftAdapterPosition.fetch(adapterPosition);
    expect(pos.owner.toBase58()).to.equal(owner.publicKey.toBase58());
    expect(pos.ifShares.toString()).to.equal("0");
    expect(pos.pendingWithdrawal).to.equal(false);

    const stakeInfo = await connection.getAccountInfo(insuranceFundStake);
    expect(stakeInfo).to.not.be.null;
    expect(stakeInfo!.owner.toBase58()).to.equal(DRIFT_PROGRAM_ID.toBase58());
  });

  // ── deposit ────────────────────────────────────────────────────────────────

  it("deposit increases IF shares and moves USDC into IF vault", async () => {
    const vaultBefore = await getAccount(connection, USDC_IF_VAULT);

    await program.methods
      .deposit(new BN(DEPOSIT_USDC))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        state: DRIFT_STATE,
        spotMarket: USDC_SPOT_MARKET,
        insuranceFundVault: USDC_IF_VAULT,
        insuranceFundStake,
        userStats,
        spotMarketVault,
        userTokenAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
        driftProgram: DRIFT_PROGRAM_ID,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).driftAdapterPosition.fetch(adapterPosition);
    expect(pos.ifShares.toString()).to.not.equal("0");

    const vaultAfter = await getAccount(connection, USDC_IF_VAULT);
    expect(Number(vaultAfter.amount)).to.be.greaterThan(Number(vaultBefore.amount));
  });

  // ── current_value ──────────────────────────────────────────────────────────

  it("current_value returns USDC value ≈ deposited amount", async () => {
    // Anchor .simulate() lacks returnData in v0.31; use connection.simulateTransaction directly.
    const tx = await program.methods
      .currentValue()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        spotMarket: USDC_SPOT_MARKET,
        insuranceFundVault: USDC_IF_VAULT,
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
    expect(value).to.be.approximately(DEPOSIT_USDC, DEPOSIT_USDC * 0.001);
  });

  // ── withdraw step 1 ────────────────────────────────────────────────────────

  it("withdraw step1: requests IF unstake and sets pending_withdrawal", async () => {
    await program.methods
      .withdraw(new BN(0))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        state: DRIFT_STATE,
        spotMarket: USDC_SPOT_MARKET,
        insuranceFundStake,
        userStats,
        insuranceFundVault: USDC_IF_VAULT,
        driftSigner: DRIFT_SIGNER,
        userTokenAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
        driftProgram: DRIFT_PROGRAM_ID,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).driftAdapterPosition.fetch(adapterPosition);
    expect(pos.pendingWithdrawal).to.equal(true);
    expect(pos.withdrawalRequestShares.toString()).to.not.equal("0");
    expect(pos.withdrawalRequestTs.toNumber()).to.be.greaterThan(0);
  });

  // ── withdraw step 2 ────────────────────────────────────────────────────────

  it("withdraw step2: completes after cooldown bypass, returns USDC to user", async () => {
    // Bypass the ~13-day cooldown by zeroing unstaking_period in the spot_market account.
    // Read the current (post-deposit) spot_market state first, then patch.
    const currentInfo = (await connection.getAccountInfo(USDC_SPOT_MARKET)) as AccountInfo<Buffer>;
    expect(currentInfo).to.not.be.null;
    await zeroSpotMarketCooldown(RPC_URL, currentInfo);

    const userAtaBefore = await getAccount(connection, userTokenAccount);

    await program.methods
      .withdraw(new BN(0))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        state: DRIFT_STATE,
        spotMarket: USDC_SPOT_MARKET,
        insuranceFundStake,
        userStats,
        insuranceFundVault: USDC_IF_VAULT,
        driftSigner: DRIFT_SIGNER,
        userTokenAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
        driftProgram: DRIFT_PROGRAM_ID,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).driftAdapterPosition.fetch(adapterPosition);
    expect(pos.pendingWithdrawal).to.equal(false);
    expect(pos.withdrawalRequestShares.toString()).to.equal("0");

    const userAtaAfter = await getAccount(connection, userTokenAccount);
    expect(Number(userAtaAfter.amount)).to.be.greaterThan(Number(userAtaBefore.amount));
    expect(Number(userAtaAfter.amount)).to.be.greaterThanOrEqual(DEPOSIT_USDC - 1);
  });
});
