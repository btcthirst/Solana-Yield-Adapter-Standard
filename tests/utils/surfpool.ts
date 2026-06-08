import { PublicKey } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";

const RPC_URL = "http://127.0.0.1:8899";

// SPL token account is exactly 165 bytes (AccountState::Initialized=1)
export function encodeTokenAccount(
  mint: PublicKey,
  owner: PublicKey,
  amount: bigint
): Buffer {
  const buf = Buffer.alloc(165);
  mint.toBuffer().copy(buf, 0);       // [0..32]  mint
  owner.toBuffer().copy(buf, 32);     // [32..64] owner
  buf.writeBigUInt64LE(amount, 64);   // [64..72] amount
  buf.writeUInt32LE(0, 72);           // [72..76] delegate_option = None
  // [76..108] delegate = zeroes
  buf.writeUInt8(1, 108);             // [108]    state = Initialized
  buf.writeUInt32LE(0, 109);          // [109..113] is_native_option = None
  // [113..121] is_native = zeroes
  buf.writeBigUInt64LE(0n, 121);      // [121..129] delegated_amount
  buf.writeUInt32LE(0, 129);          // [129..133] close_authority_option = None
  // [133..165] close_authority = zeroes
  return buf;
}

// Inject an SPL token account into the surfpool simnet with a given balance.
// Uses the surfnet_setAccount JSON-RPC method available in surfpool.
export async function fundTokenAccount(
  tokenAccount: PublicKey,
  mint: PublicKey,
  owner: PublicKey,
  amount: bigint
): Promise<void> {
  const data = encodeTokenAccount(mint, owner, amount);
  await rpcCall("surfnet_setAccount", [
    {
      pubkey: tokenAccount.toBase58(),
      account: {
        lamports: 2_039_280,
        data: [data.toString("base64"), "base64"],
        owner: TOKEN_PROGRAM_ID.toBase58(),
        executable: false,
        rentEpoch: 0,
      },
    },
  ]);
}

async function rpcCall(method: string, params: unknown[]): Promise<unknown> {
  const res = await fetch(RPC_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const json = (await res.json()) as { result?: unknown; error?: unknown };
  if (json.error) throw new Error(`${method} failed: ${JSON.stringify(json.error)}`);
  return json.result;
}
