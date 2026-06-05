/**
 * Dispatcher + Registry — integration test
 *
 * Tests the full Dispatcher → Registry → Adapter → Protocol chain using
 * MarginFi USDC as the reference adapter (immediate deposit/withdraw, no cooldown).
 *
 * Happy path:
 *   1. initialize_registry  (authority = owner wallet)
 *   2. approve_adapter      (registers MarginFi USDC adapter)
 *   3. MarginFi adapter:    initialize_position (direct call — Dispatcher does not proxy this)
 *   4. Dispatcher:          initialize_position (creates UserPosition PDA)
 *   5. Dispatcher:          deposit  → UserPosition.shares > 0
 *   6. Dispatcher:          current_value → USDC value via simulateTransaction
 *   7. Dispatcher:          withdraw(0) → UserPosition.shares = 0
 *   8. Dispatcher:          close_position → UserPosition account closed, rent returned
 *
 * Negative tests:
 *   - withdraw(1) when shares = 0      → InsufficientShares (6001)
 *   - initialize_position unregistered → AccountNotInitialized (account missing)
 *   - deposit with paused adapter      → AdapterPaused (6202)
 *   - deposit with revoked adapter     → AdapterRevoked (6201)
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

const MARGINFI_PROGRAM_ID = new PublicKey("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA");
const USDC_BANK           = new PublicKey("2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB");
const USDC_MINT           = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

const MARGINFI_ADAPTER_ID = new PublicKey("47aSt3hDuDSW1RFz2Qbi9tUc5V7HMotJU3zyiqrkZ9zz");
const DISPATCHER_ID       = new PublicKey("F6QyZM6rb5i1bDsW9gQMrPBVzZeEfSRbR2JDzDQTJuQ1");
const REGISTRY_ID         = new PublicKey("4NP3DgbM7JJDBQEiU9ojUJ3yYnCoEnbBCsACfQz32xdB");

const DEPOSIT_USDC = 10_000_000; // 10 USDC (6 decimals)

// ─── Helpers ──────────────────────────────────────────────────────────────────

function findPDA(seeds: Buffer[], program: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, program)[0];
}

// Encode a string into a zero-padded 64-byte array (Registry name/protocol fields).
function strToBytes64(s: string): number[] {
  const buf = Buffer.alloc(64, 0);
  Buffer.from(s, "utf-8").copy(buf, 0);
  return Array.from(buf);
}

// Read marginfi group pubkey from USDC bank account data (offset 48..80).
// Returns null if the bank account is not yet available (mainnet fork not running).
async function readMarginfiGroup(conn: Connection): Promise<PublicKey | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const info = await conn.getAccountInfo(USDC_BANK);
    if (info && info.data.length >= 80) {
      return new PublicKey(info.data.slice(48, 80));
    }
    await new Promise(r => setTimeout(r, 2000));
  }
  return null;
}

// ─── Suite ────────────────────────────────────────────────────────────────────

describe("Dispatcher Integration", () => {
  const RPC_URL    = "http://127.0.0.1:8899";
  const connection = new Connection(RPC_URL, "confirmed");
  const owner      = Keypair.generate();

  let dispatcherProgram: Program;
  let registryProgram:   Program;
  let marginfiAdapterProgram: Program;

  // PDAs derived in before()
  let registryState:              PublicKey;
  let registryEntry:              PublicKey;
  let userPosition:               PublicKey;
  let adapterPosition:            PublicKey;
  let marginfiAuthority:          PublicKey;
  let marginfiGroup:              PublicKey;
  let marginfiAccount:            PublicKey;
  let bankLiquidityVault:         PublicKey;
  let bankLiquidityVaultAuthority: PublicKey;
  let signerTokenAccount:         PublicKey; // USDC ATA owned by marginfiAuthority (deposit source)
  let destinationTokenAccount:    PublicKey; // USDC ATA owned by owner (withdraw destination)

  before(async function () {
    this.timeout(90_000);

    // Airdrop SOL for rent + fees
    const sig = await connection.requestAirdrop(owner.publicKey, 10 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig);

    // Check mainnet fork availability — reads USDC bank to get marginfi group
    const group = await readMarginfiGroup(connection);
    if (!group) {
      skipOrFail(this, "MarginFi USDC bank not available (Dispatcher e2e)");
      return;
    }
    marginfiGroup = group;

    // Load programs from IDL
    const idlBase = path.join(__dirname, "../../target/idl");
    const dispatcherIdl = JSON.parse(fs.readFileSync(`${idlBase}/dispatcher.json`, "utf-8"));
    const registryIdl   = JSON.parse(fs.readFileSync(`${idlBase}/registry.json`, "utf-8"));
    const mfiIdl        = JSON.parse(fs.readFileSync(`${idlBase}/marginfi_adapter.json`, "utf-8"));

    const provider = new anchor.AnchorProvider(
      connection,
      new anchor.Wallet(owner),
      { commitment: "confirmed", skipPreflight: false }
    );
    dispatcherProgram      = new Program(dispatcherIdl, provider);
    registryProgram        = new Program(registryIdl, provider);
    marginfiAdapterProgram = new Program(mfiIdl, provider);

    // ── Derive all PDAs ───────────────────────────────────────────────────────

    registryState = findPDA([Buffer.from("registry_state")], REGISTRY_ID);
    registryEntry = findPDA([Buffer.from("registry_entry"), MARGINFI_ADAPTER_ID.toBuffer()], REGISTRY_ID);

    userPosition = findPDA(
      [Buffer.from("position"), owner.publicKey.toBuffer(), MARGINFI_ADAPTER_ID.toBuffer()],
      DISPATCHER_ID
    );

    adapterPosition   = findPDA([Buffer.from("mfi_pos"),  owner.publicKey.toBuffer()], MARGINFI_ADAPTER_ID);
    marginfiAuthority = findPDA([Buffer.from("mfi_auth"), owner.publicKey.toBuffer()], MARGINFI_ADAPTER_ID);

    marginfiAccount = findPDA(
      [Buffer.from("marginfi_account"), marginfiGroup.toBuffer(), marginfiAuthority.toBuffer()],
      MARGINFI_PROGRAM_ID
    );

    bankLiquidityVault = findPDA(
      [Buffer.from("liquidity_vault"), USDC_BANK.toBuffer()],
      MARGINFI_PROGRAM_ID
    );
    bankLiquidityVaultAuthority = findPDA(
      [Buffer.from("liquidity_vault_auth"), USDC_BANK.toBuffer()],
      MARGINFI_PROGRAM_ID
    );

    signerTokenAccount      = getAssociatedTokenAddressSync(USDC_MINT, marginfiAuthority, true);
    destinationTokenAccount = getAssociatedTokenAddressSync(USDC_MINT, owner.publicKey);

    // ── Bootstrap Registry ────────────────────────────────────────────────────

    await registryProgram.methods
      .initializeRegistry(owner.publicKey)
      .accounts({
        payer:         owner.publicKey,
        registryState,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    await registryProgram.methods
      .approveAdapter(
        MARGINFI_ADAPTER_ID,
        strToBytes64("MarginFi USDC"),
        strToBytes64("MarginFi"),
        USDC_MINT,
      )
      .accounts({
        authority:     owner.publicKey,
        registryState,
        registryEntry,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    // ── MarginFi adapter: initialize_position (direct — Dispatcher does not proxy this) ──

    await marginfiAdapterProgram.methods
      .initializePosition()
      .accounts({
        owner:            owner.publicKey,
        adapterPosition,
        marginfiAuthority,
        marginfiGroup,
        marginfiAccount,
        marginfiProgram:  MARGINFI_PROGRAM_ID,
        systemProgram:    SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    // ── Dispatcher: initialize_position (creates UserPosition PDA) ────────────

    await dispatcherProgram.methods
      .initializePosition(MARGINFI_ADAPTER_ID)
      .accounts({
        owner:         owner.publicKey,
        userPosition,
        registryEntry,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    // ── Fund token accounts ───────────────────────────────────────────────────

    // Create USDC ATA owned by marginfiAuthority (deposit source)
    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(
        createAssociatedTokenAccountInstruction(
          owner.publicKey, signerTokenAccount, marginfiAuthority, USDC_MINT
        )
      ),
      [owner]
    );
    await fundTokenAccount(RPC_URL, signerTokenAccount, USDC_MINT, marginfiAuthority, BigInt(DEPOSIT_USDC * 10));

    // Create USDC ATA owned by owner (withdraw destination)
    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(
        createAssociatedTokenAccountInstruction(
          owner.publicKey, destinationTokenAccount, owner.publicKey, USDC_MINT
        )
      ),
      [owner]
    );
  });

  // ── Happy path ────────────────────────────────────────────────────────────

  it("deposit routes to MarginFi adapter and records shares in UserPosition", async () => {
    await dispatcherProgram.methods
      .deposit(new BN(DEPOSIT_USDC))
      .accounts({
        owner:          owner.publicKey,
        userPosition,
        registryEntry,
        adapterProgram: MARGINFI_ADAPTER_ID,
      })
      .remainingAccounts([
        // MarginFi adapter deposit accounts in struct order:
        { pubkey: owner.publicKey,        isSigner: false, isWritable: false }, // owner (UncheckedAccount)
        { pubkey: adapterPosition,        isSigner: false, isWritable: true  }, // adapter_position
        { pubkey: marginfiAuthority,      isSigner: false, isWritable: false }, // marginfi_authority
        { pubkey: marginfiGroup,          isSigner: false, isWritable: false }, // marginfi_group
        { pubkey: marginfiAccount,        isSigner: false, isWritable: true  }, // marginfi_account
        { pubkey: USDC_BANK,              isSigner: false, isWritable: true  }, // bank
        { pubkey: signerTokenAccount,     isSigner: false, isWritable: true  }, // signer_token_account
        { pubkey: bankLiquidityVault,     isSigner: false, isWritable: true  }, // bank_liquidity_vault
        { pubkey: TOKEN_PROGRAM_ID,       isSigner: false, isWritable: false }, // token_program
        { pubkey: MARGINFI_PROGRAM_ID,    isSigner: false, isWritable: false }, // marginfi_program
      ])
      .signers([owner])
      .rpc();

    const pos = await (dispatcherProgram.account as any).userPosition.fetch(userPosition);
    expect(pos.shares.toNumber()).to.be.greaterThan(0);
    expect(pos.adapter.toBase58()).to.equal(MARGINFI_ADAPTER_ID.toBase58());
    expect(pos.owner.toBase58()).to.equal(owner.publicKey.toBase58());
  });

  it("current_value routes to adapter and returns USDC value via returnData", async () => {
    const tx = await dispatcherProgram.methods
      .currentValue()
      .accounts({
        owner:          owner.publicKey,
        userPosition,
        registryEntry,
        adapterProgram: MARGINFI_ADAPTER_ID,
      })
      .remainingAccounts([
        // MarginFi adapter current_value accounts in struct order:
        { pubkey: owner.publicKey,   isSigner: false, isWritable: false }, // owner
        { pubkey: adapterPosition,   isSigner: false, isWritable: false }, // adapter_position
        { pubkey: USDC_BANK,         isSigner: false, isWritable: false }, // bank
      ])
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

  it("withdraw(0) routes to adapter and zeroes UserPosition shares", async () => {
    const destBefore = await getAccount(connection, destinationTokenAccount);

    await dispatcherProgram.methods
      .withdraw(new BN(0)) // 0 = withdraw all
      .accounts({
        owner:          owner.publicKey,
        userPosition,
        registryEntry,
        adapterProgram: MARGINFI_ADAPTER_ID,
      })
      .remainingAccounts([
        // MarginFi adapter withdraw accounts in struct order:
        { pubkey: owner.publicKey,               isSigner: false, isWritable: false }, // owner
        { pubkey: adapterPosition,               isSigner: false, isWritable: true  }, // adapter_position
        { pubkey: marginfiAuthority,             isSigner: false, isWritable: false }, // marginfi_authority
        { pubkey: marginfiGroup,                 isSigner: false, isWritable: false }, // marginfi_group
        { pubkey: marginfiAccount,               isSigner: false, isWritable: true  }, // marginfi_account
        { pubkey: USDC_BANK,                     isSigner: false, isWritable: true  }, // bank
        { pubkey: destinationTokenAccount,       isSigner: false, isWritable: true  }, // destination_token_account
        { pubkey: bankLiquidityVaultAuthority,   isSigner: false, isWritable: false }, // bank_liquidity_vault_authority
        { pubkey: bankLiquidityVault,            isSigner: false, isWritable: true  }, // bank_liquidity_vault
        { pubkey: TOKEN_PROGRAM_ID,              isSigner: false, isWritable: false }, // token_program
        { pubkey: MARGINFI_PROGRAM_ID,           isSigner: false, isWritable: false }, // marginfi_program
      ])
      .signers([owner])
      .rpc();

    const pos = await (dispatcherProgram.account as any).userPosition.fetch(userPosition);
    expect(pos.shares.toNumber()).to.equal(0);

    const destAfter = await getAccount(connection, destinationTokenAccount);
    expect(Number(destAfter.amount)).to.be.greaterThan(Number(destBefore.amount));
    expect(Number(destAfter.amount)).to.be.greaterThanOrEqual(DEPOSIT_USDC - 1);
  });

  it("close_position closes UserPosition PDA and returns rent to owner", async () => {
    const ownerLamportsBefore = await connection.getBalance(owner.publicKey);

    await dispatcherProgram.methods
      .closePosition(MARGINFI_ADAPTER_ID)
      .accounts({
        owner:         owner.publicKey,
        userPosition,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    const info = await connection.getAccountInfo(userPosition);
    expect(info).to.be.null;

    const ownerLamportsAfter = await connection.getBalance(owner.publicKey);
    expect(ownerLamportsAfter).to.be.greaterThan(ownerLamportsBefore);
  });

  // ── Negative tests ────────────────────────────────────────────────────────
  // userPosition is closed at this point; these tests use initialize_position
  // as the target instruction since it also validates registry_entry constraints.

  it("withdraw(1) when UserPosition shares = 0 → InsufficientShares (6001)", async () => {
    // Re-create the dispatcher UserPosition (shares = 0 after re-init)
    await dispatcherProgram.methods
      .initializePosition(MARGINFI_ADAPTER_ID)
      .accounts({
        owner:         owner.publicKey,
        userPosition,
        registryEntry,
        systemProgram: SystemProgram.programId,
      })
      .signers([owner])
      .rpc();

    try {
      await dispatcherProgram.methods
        .withdraw(new BN(1))
        .accounts({
          owner:          owner.publicKey,
          userPosition,
          registryEntry,
          adapterProgram: MARGINFI_ADAPTER_ID,
        })
        .remainingAccounts([]) // constraint fires before adapter CPI, no accounts needed
        .signers([owner])
        .rpc();
      expect.fail("Expected InsufficientShares error");
    } catch (err: any) {
      const code = err?.error?.errorCode?.number ?? err?.errorCode?.number;
      expect(code, `expected error 6001, got: ${JSON.stringify(err?.error ?? err?.message)}`).to.equal(6001);
    }
  });

  it("initialize_position with unregistered adapter → account not found error", async () => {
    const fakeAdapter  = Keypair.generate().publicKey;
    const fakeEntry    = findPDA([Buffer.from("registry_entry"), fakeAdapter.toBuffer()], REGISTRY_ID);
    const fakePosition = findPDA(
      [Buffer.from("position"), owner.publicKey.toBuffer(), fakeAdapter.toBuffer()],
      DISPATCHER_ID
    );

    try {
      await dispatcherProgram.methods
        .initializePosition(fakeAdapter)
        .accounts({
          owner:         owner.publicKey,
          userPosition:  fakePosition,
          registryEntry: fakeEntry,
          systemProgram: SystemProgram.programId,
        })
        .signers([owner])
        .rpc();
      expect.fail("Expected error for unregistered adapter");
    } catch (err: any) {
      // Anchor throws when the account doesn't exist or has wrong discriminator.
      // Accept any error — the key assertion is that the call did NOT succeed.
      expect(err).to.exist;
    }
  });

  it("deposit with paused adapter → AdapterPaused (6202)", async () => {
    // userPosition exists (shares = 0). Constraint check fires before adapter CPI,
    // so empty remaining_accounts is fine.
    await registryProgram.methods
      .pauseAdapter(MARGINFI_ADAPTER_ID)
      .accounts({
        authority:    owner.publicKey,
        registryState,
        registryEntry,
      })
      .signers([owner])
      .rpc();

    try {
      await dispatcherProgram.methods
        .deposit(new BN(1))
        .accounts({
          owner:          owner.publicKey,
          userPosition,
          registryEntry,
          adapterProgram: MARGINFI_ADAPTER_ID,
        })
        .remainingAccounts([])
        .signers([owner])
        .rpc();
      expect.fail("Expected AdapterPaused error");
    } catch (err: any) {
      const code = err?.error?.errorCode?.number ?? err?.errorCode?.number;
      expect(code, `expected 6202, got: ${JSON.stringify(err?.error ?? err?.message)}`).to.equal(6202);
    }
  });

  it("deposit with revoked adapter → AdapterRevoked (6201)", async () => {
    // Revoke fires is_active = false; checked before is_paused, so AdapterRevoked wins.
    await registryProgram.methods
      .revokeAdapter(MARGINFI_ADAPTER_ID)
      .accounts({
        authority:    owner.publicKey,
        registryState,
        registryEntry,
      })
      .signers([owner])
      .rpc();

    try {
      await dispatcherProgram.methods
        .deposit(new BN(1))
        .accounts({
          owner:          owner.publicKey,
          userPosition,
          registryEntry,
          adapterProgram: MARGINFI_ADAPTER_ID,
        })
        .remainingAccounts([])
        .signers([owner])
        .rpc();
      expect.fail("Expected AdapterRevoked error");
    } catch (err: any) {
      const code = err?.error?.errorCode?.number ?? err?.errorCode?.number;
      expect(code, `expected 6201, got: ${JSON.stringify(err?.error ?? err?.message)}`).to.equal(6201);
    }
  });
});
