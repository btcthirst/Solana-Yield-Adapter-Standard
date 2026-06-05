/**
 * Kamino Lending USDC Adapter — mainnet-fork integration test
 *
 * Prerequisites:
 *   surfpool start --network mainnet --no-tui --daemon
 *
 * Then run:
 *   npm run test:fork -- --grep "Kamino"
 */

import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  SYSVAR_INSTRUCTIONS_PUBKEY,
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
import * as fs from "fs";
import * as path from "path";

// ─── surfpool helper (inlined to avoid cross-dir TS module resolution issues) ─

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

const KLEND_PROGRAM_ID    = new PublicKey("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD");
const LENDING_MARKET      = new PublicKey("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF");
// USDC Reserve in Kamino main market — mint/supply vaults read from account data at runtime
const USDC_RESERVE        = new PublicKey("D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59");
const USDC_MINT           = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const ADAPTER_PROGRAM_ID  = new PublicKey("5ksJ5dU6jAoZaUnpcXtGN69xXewcRcGLTBisQHSkwc44");

const DEPOSIT_USDC = 10_000_000; // 10 USDC (6 decimals)

// Reserve layout offsets — see programs/kamino-adapter/src/cpi.rs
const OFF_LIQUIDITY_MINT    = 128;
const OFF_LIQUIDITY_SUPPLY  = 160;
const OFF_COLLATERAL_MINT   = 2560;
const OFF_COLLATERAL_SUPPLY = 2600; // supplyVault in ReserveCollateral

// ─── Helpers ──────────────────────────────────────────────────────────────────

function findPDA(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}

interface ReserveAccounts {
  liquidityMint: PublicKey;
  liquiditySupply: PublicKey;
  collateralMint: PublicKey;
  collateralSupply: PublicKey;
}

// Read Kamino reserve vault/mint addresses from on-chain reserve account data.
// Returns null if unavailable (mainnet fork not running or wrong reserve address).
async function readReserve(conn: Connection, reserve: PublicKey): Promise<ReserveAccounts | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const info = await conn.getAccountInfo(reserve);
    if (info && info.data.length >= OFF_COLLATERAL_SUPPLY + 32) {
      const d = info.data;
      const liquidityMint   = new PublicKey(d.slice(OFF_LIQUIDITY_MINT,    OFF_LIQUIDITY_MINT + 32));
      const liquiditySupply = new PublicKey(d.slice(OFF_LIQUIDITY_SUPPLY,  OFF_LIQUIDITY_SUPPLY + 32));
      const collateralMint  = new PublicKey(d.slice(OFF_COLLATERAL_MINT,   OFF_COLLATERAL_MINT + 32));
      const collateralSupply = new PublicKey(d.slice(OFF_COLLATERAL_SUPPLY, OFF_COLLATERAL_SUPPLY + 32));
      // Sanity-check: liquidity mint must be USDC
      if (liquidityMint.toBase58() !== USDC_MINT.toBase58()) {
        console.log(`    ⚠  Reserve liquidity mint mismatch: got ${liquidityMint.toBase58()}`);
        return null;
      }
      return { liquidityMint, liquiditySupply, collateralMint, collateralSupply };
    }
    await new Promise(r => setTimeout(r, 2000));
  }
  return null;
}

// ─── Suite ────────────────────────────────────────────────────────────────────

describe("Kamino Adapter", () => {
  const connection = new Connection("http://127.0.0.1:8899", "confirmed");
  const owner = Keypair.generate();

  // Adapter PDAs
  const adapterPosition = findPDA(
    [Buffer.from("k_pos"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );
  const kaminoAuthority = findPDA(
    [Buffer.from("k_auth"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );

  // Kamino-program PDAs (derivable without mainnet data)
  //   lending_market_authority seeds: [b"lma", lending_market] @ KLEND_PROGRAM_ID
  const lendingMarketAuthority = findPDA(
    [Buffer.from("lma"), LENDING_MARKET.toBuffer()],
    KLEND_PROGRAM_ID
  );
  //   obligation seeds: [tag=0, id=0, authority, market, sys, sys] @ KLEND_PROGRAM_ID
  const obligation = findPDA(
    [
      Buffer.from([0]),
      Buffer.from([0]),
      kaminoAuthority.toBuffer(),
      LENDING_MARKET.toBuffer(),
      SystemProgram.programId.toBuffer(),
      SystemProgram.programId.toBuffer(),
    ],
    KLEND_PROGRAM_ID
  );
  //   user_metadata seeds: [b"user_meta", authority] @ KLEND_PROGRAM_ID
  const ownerUserMetadata = findPDA(
    [Buffer.from("user_meta"), kaminoAuthority.toBuffer()],
    KLEND_PROGRAM_ID
  );

  // Token accounts
  // Deposit source: ATA owned by kaminoAuthority PDA (signs the deposit CPI)
  const userSourceLiquidity = getAssociatedTokenAddressSync(USDC_MINT, kaminoAuthority, true);
  // Withdraw destination: user's own USDC ATA
  const userDestinationLiquidity = getAssociatedTokenAddressSync(USDC_MINT, owner.publicKey);

  let program: Program;
  let ra: ReserveAccounts;

  before(async function () {
    this.timeout(60_000);

    const sig = await connection.requestAirdrop(owner.publicKey, 10 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig);

    // Validate mainnet fork is up and USDC_RESERVE is correct
    const reserve = await readReserve(connection, USDC_RESERVE);
    if (!reserve) {
      console.log("    ⚠  Kamino USDC reserve not available — mainnet fork required. Skipping all tests.");
      console.log("       Run with: SURFPOOL_DATASOURCE_RPC_URL=<mainnet-rpc-url> anchor test --skip-build");
      this.skip();
      return;
    }
    ra = reserve;

    const idlPath = path.join(__dirname, "../../target/idl/kamino_adapter.json");
    const idl = JSON.parse(fs.readFileSync(idlPath, "utf-8"));
    const provider = new anchor.AnchorProvider(
      connection,
      new anchor.Wallet(owner),
      { commitment: "confirmed", skipPreflight: false }
    );
    program = new Program(idl, provider);

    // Create kaminoAuthority's USDC ATA and fund it
    const createSourceAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        owner.publicKey,
        userSourceLiquidity,
        kaminoAuthority,
        USDC_MINT
      )
    );
    await sendAndConfirmTransaction(connection, createSourceAta, [owner]);
    await fundTokenAccount(
      "http://127.0.0.1:8899",
      userSourceLiquidity,
      USDC_MINT,
      kaminoAuthority,
      BigInt(DEPOSIT_USDC * 10) // 100 USDC — enough for multiple runs
    );

    // Create user's receive ATA for withdraw
    const createDestAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        owner.publicKey,
        userDestinationLiquidity,
        owner.publicKey,
        USDC_MINT
      )
    );
    await sendAndConfirmTransaction(connection, createDestAta, [owner]);
  });

  // ── initialize_position ────────────────────────────────────────────────────

  it("initialize_position creates adapter position and Kamino obligation", async () => {
    await program.methods
      .initializePosition()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        kaminoAuthority,
        lendingMarket: LENDING_MARKET,
        ownerUserMetadata,
        obligation,
        reserve: USDC_RESERVE,
        klendProgram: KLEND_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).kaminoAdapterPosition.fetch(adapterPosition);
    expect(pos.owner.toBase58()).to.equal(owner.publicKey.toBase58());
    expect(pos.shares.toNumber()).to.equal(0);
    expect(pos.obligation.toBase58()).to.equal(obligation.toBase58());
    expect(pos.reserve.toBase58()).to.equal(USDC_RESERVE.toBase58());

    const oblInfo = await connection.getAccountInfo(obligation);
    expect(oblInfo).to.not.be.null;
    expect(oblInfo!.owner.toBase58()).to.equal(KLEND_PROGRAM_ID.toBase58());
  });

  // ── deposit ────────────────────────────────────────────────────────────────

  it("deposit increases shares and moves USDC into Kamino reserve", async () => {
    const supplyBefore = await getAccount(connection, ra.liquiditySupply);

    await program.methods
      .deposit(new BN(DEPOSIT_USDC))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        kaminoAuthority,
        lendingMarket: LENDING_MARKET,
        lendingMarketAuthority,
        obligation,
        reserve: USDC_RESERVE,
        reserveLiquidityMint: ra.liquidityMint,
        reserveLiquiditySupply: ra.liquiditySupply,
        reserveCollateralMint: ra.collateralMint,
        reserveDestinationDepositCollateral: ra.collateralSupply,
        userSourceLiquidity,
        tokenProgram: TOKEN_PROGRAM_ID,
        instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
        klendProgram: KLEND_PROGRAM_ID,
      })
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
      ])
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).kaminoAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.be.greaterThan(0);

    const supplyAfter = await getAccount(connection, ra.liquiditySupply);
    expect(Number(supplyAfter.amount)).to.be.greaterThan(Number(supplyBefore.amount));
  });

  // ── current_value ──────────────────────────────────────────────────────────

  it("current_value returns USDC value ≈ deposited amount", async () => {
    // Anchor .simulate() lacks returnData in v0.31; use connection.simulateTransaction directly.
    const tx = await program.methods
      .currentValue()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        lendingMarket: LENDING_MARKET,
        reserve: USDC_RESERVE,
        klendProgram: KLEND_PROGRAM_ID,
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

  // ── withdraw all ───────────────────────────────────────────────────────────

  it("withdraw(0) returns all USDC to user and zeroes shares", async () => {
    await program.methods
      .withdraw(new BN(0)) // 0 = withdraw all (u64::MAX ctokens)
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        kaminoAuthority,
        lendingMarket: LENDING_MARKET,
        lendingMarketAuthority,
        obligation,
        reserve: USDC_RESERVE,
        reserveLiquidityMint: ra.liquidityMint,
        reserveSourceCollateral: ra.collateralSupply,
        reserveCollateralMint: ra.collateralMint,
        reserveLiquiditySupply: ra.liquiditySupply,
        userDestinationLiquidity,
        tokenProgram: TOKEN_PROGRAM_ID,
        instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
        klendProgram: KLEND_PROGRAM_ID,
      })
      .preInstructions([
        ComputeBudgetProgram.setComputeUnitLimit({ units: 500_000 }),
      ])
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).kaminoAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.equal(0);

    const dest = await getAccount(connection, userDestinationLiquidity);
    expect(Number(dest.amount)).to.be.greaterThanOrEqual(DEPOSIT_USDC - 1);
  });
});
