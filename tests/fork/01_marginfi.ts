/**
 * MarginFi USDC Adapter — mainnet-fork integration test
 *
 * Prerequisites:
 *   surfpool start --network mainnet --no-tui --daemon
 *
 * Then run:
 *   npm run test:fork -- --grep "MarginFi"
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

const MARGINFI_PROGRAM_ID = new PublicKey("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA");
// USDC bank address — group is read dynamically from bank account data (offset 48..80)
const USDC_BANK           = new PublicKey("2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB");
const USDC_MINT           = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const ADAPTER_PROGRAM_ID  = new PublicKey("47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz"); // marginfi-adapter

const DEPOSIT_USDC = 10_000_000; // 10 USDC (6 decimals)

// ─── Helpers ──────────────────────────────────────────────────────────────────

function findPDA(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}

// Read marginfi group pubkey from bank account data (offset 48..80, after 8-byte disc).
// Surfpool fetches mainnet accounts lazily; retry until available.
// Returns null if not available (test will skip).
async function readMarginfiGroup(conn: Connection): Promise<PublicKey | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const info = await conn.getAccountInfo(USDC_BANK);
    if (info && info.data.length >= 80) {
      const groupBytes = info.data.slice(48, 80);
      return new PublicKey(groupBytes);
    }
    await new Promise(r => setTimeout(r, 2000));
  }
  return null; // mainnet fork not available, tests will be skipped
}

// ─── Suite ────────────────────────────────────────────────────────────────────

describe("MarginFi Adapter", () => {
  const connection = new Connection("http://127.0.0.1:8899", "confirmed");
  const owner = Keypair.generate();

  // Adapter PDAs (computable without group)
  const adapterPosition = findPDA(
    [Buffer.from("mfi_pos"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );
  const marginfiAuthority = findPDA(
    [Buffer.from("mfi_auth"), owner.publicKey.toBuffer()],
    ADAPTER_PROGRAM_ID
  );

  // Bank vault PDAs (under marginfi program — derivable from bank key alone)
  const bankLiquidityVault = findPDA(
    [Buffer.from("liquidity_vault"), USDC_BANK.toBuffer()],
    MARGINFI_PROGRAM_ID
  );
  const bankLiquidityVaultAuthority = findPDA(
    [Buffer.from("liquidity_vault_auth"), USDC_BANK.toBuffer()],
    MARGINFI_PROGRAM_ID
  );

  // Token accounts
  // Deposit source: ATA owned by the adapter PDA (not user wallet),
  // so the PDA can sign the MarginFi deposit CPI.
  const signerTokenAccount = getAssociatedTokenAddressSync(
    USDC_MINT,
    marginfiAuthority,
    true // allowOwnerOffCurve — PDA is off-curve
  );
  // Withdraw destination: user's own USDC ATA
  const destinationTokenAccount = getAssociatedTokenAddressSync(USDC_MINT, owner.publicKey);

  let program: Program;
  // marginfiGroup and marginfiAccount are derived in before() after reading bank data
  let marginfiGroup: PublicKey;
  let marginfiAccount: PublicKey;
  let forkAvailable = false;

  before(async function () {
    // Fund owner with SOL
    const sig = await connection.requestAirdrop(owner.publicKey, 10 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig);

    // Read the MarginFi group from the bank account (offset 48..80)
    const group = await readMarginfiGroup(connection);
    if (!group) {
      console.log("    ⚠  USDC bank not available — mainnet fork required. Skipping all tests.");
      console.log("       Run with: SURFPOOL_DATASOURCE_RPC_URL=<mainnet-rpc-url> anchor test --skip-build");
      this.skip();
      return;
    }
    forkAvailable = true;
    marginfiGroup = group;
    marginfiAccount = findPDA(
      [
        Buffer.from("marginfi_account"),
        marginfiGroup.toBuffer(),
        marginfiAuthority.toBuffer(),
      ],
      MARGINFI_PROGRAM_ID
    );

    // Load adapter IDL and create program client
    const idlPath = path.join(__dirname, "../../target/idl/marginfi_adapter.json");
    const idl = JSON.parse(fs.readFileSync(idlPath, "utf-8"));
    const provider = new anchor.AnchorProvider(
      connection,
      new anchor.Wallet(owner),
      { commitment: "confirmed", skipPreflight: false }
    );
    program = new Program(idl, provider);

    // Create ATA owned by marginfiAuthority (deposit staging account)
    const createSignerAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        owner.publicKey,
        signerTokenAccount,
        marginfiAuthority,
        USDC_MINT
      )
    );
    await sendAndConfirmTransaction(connection, createSignerAta, [owner]);

    // Inject USDC balance into the PDA-owned ATA via surfpool
    await fundTokenAccount(
      "http://127.0.0.1:8899",
      signerTokenAccount,
      USDC_MINT,
      marginfiAuthority,
      BigInt(DEPOSIT_USDC * 10) // 100 USDC — enough for multiple test runs
    );

    // Create user's receive ATA for withdraw
    const createDestAta = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        owner.publicKey,
        destinationTokenAccount,
        owner.publicKey,
        USDC_MINT
      )
    );
    await sendAndConfirmTransaction(connection, createDestAta, [owner]);
  });

  // ── initialize_position ────────────────────────────────────────────────────

  it("initialize_position creates adapter position and MarginFi account", async () => {
    await program.methods
      .initializePosition()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        marginfiAuthority,
        marginfiGroup,
        marginfiAccount,
        marginfiProgram: MARGINFI_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).marginfiAdapterPosition.fetch(adapterPosition);
    expect(pos.owner.toBase58()).to.equal(owner.publicKey.toBase58());
    expect(pos.shares.toNumber()).to.equal(0);
    expect(pos.marginfiAccount.toBase58()).to.equal(marginfiAccount.toBase58());

    // MarginFi account should exist on-chain now
    const mfiAccInfo = await connection.getAccountInfo(marginfiAccount);
    expect(mfiAccInfo).to.not.be.null;
    expect(mfiAccInfo!.owner.toBase58()).to.equal(MARGINFI_PROGRAM_ID.toBase58());
  });

  // ── deposit ────────────────────────────────────────────────────────────────

  it("deposit increases shares and moves USDC into MarginFi vault", async () => {
    const vaultBefore = await getAccount(connection, bankLiquidityVault);

    await program.methods
      .deposit(new BN(DEPOSIT_USDC))
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        marginfiAuthority,
        marginfiGroup,
        marginfiAccount,
        bank: USDC_BANK,
        signerTokenAccount,
        bankLiquidityVault,
        tokenProgram: TOKEN_PROGRAM_ID,
        marginfiProgram: MARGINFI_PROGRAM_ID,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).marginfiAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.be.greaterThan(0);

    const vaultAfter = await getAccount(connection, bankLiquidityVault);
    expect(Number(vaultAfter.amount)).to.be.greaterThan(Number(vaultBefore.amount));
  });

  // ── current_value ──────────────────────────────────────────────────────────

  it("current_value returns USDC value ≈ deposited amount", async () => {
    // Anchor's .simulate() doesn't surface returnData; use connection directly.
    const tx = await program.methods
      .currentValue()
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        bank: USDC_BANK,
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
      .withdraw(new BN(0)) // 0 = withdraw all
      .accounts({
        owner: owner.publicKey,
        adapterPosition,
        marginfiAuthority,
        marginfiGroup,
        marginfiAccount,
        bank: USDC_BANK,
        destinationTokenAccount,
        bankLiquidityVaultAuthority,
        bankLiquidityVault,
        tokenProgram: TOKEN_PROGRAM_ID,
        marginfiProgram: MARGINFI_PROGRAM_ID,
      })
      .signers([owner])
      .rpc();

    const pos = await (program.account as any).marginfiAdapterPosition.fetch(adapterPosition);
    expect(pos.shares.toNumber()).to.equal(0);

    const dest = await getAccount(connection, destinationTokenAccount);
    // Should receive ≥ deposited amount (may include accrued interest)
    expect(Number(dest.amount)).to.be.greaterThanOrEqual(DEPOSIT_USDC - 1);
  });
});
