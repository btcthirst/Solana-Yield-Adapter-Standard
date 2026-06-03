# План: Mainnet-Fork Tests

## Мета

Інтеграційні тести для всіх п'яти адаптерів проти реального mainnet state. Використовуємо Surfpool для локального форкування mainnet.

---

## Інструментарій

### Surfpool (основний)
Surfpool — локальна Solana нода що форкує mainnet стан і дозволяє:
- Клонувати будь-які mainnet акаунти
- Виконувати транзакції проти реального стану
- Модифікувати стан (cheatcodes) для тестових сценаріїв

```bash
# Встановлення
cargo install surfpool

# Запуск з mainnet fork
NO_DNA=1 surfpool start --fork mainnet-beta \
  --rpc-url https://mainnet.helius-rpc.com/?api-key=<KEY> \
  --accounts <comma_separated_accounts_to_clone>
```

### LiteSVM (unit тести)
Для швидких unit тестів без fork — завантаження програм та акаунтів вручну.

### Mocha + Anchor TS (test runner)
TypeScript тести через `@coral-xyz/anchor` test framework.

---

## Архітектура тестів

```
tests/
├── setup/
│   ├── surfpool.ts          — запуск Surfpool та seed акаунтів
│   ├── accounts.ts          — адреси всіх mainnet акаунтів
│   └── helpers.ts           — mintUsdc, createATA, advanceClock, etc
├── adapters/
│   ├── marginfi.test.ts
│   ├── drift.test.ts
│   ├── kamino.test.ts
│   ├── jupiter-lp.test.ts
│   └── maple.test.ts
└── integration/
    └── dispatcher.test.ts   — тести через Dispatcher → Adapter → Protocol
```

---

## Список акаунтів для клонування

### MarginFi
```
MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA  # program
4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG  # group
2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB  # USDC bank
EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v  # USDC mint
```

### Drift
```
dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH   # program
FxztXFc4aiyHzGHCTzJRys5X4TnWVBzJTYeSMn7b65nA  # state
<usdc_spot_market>                               # USDC spot market
<if_vault_usdc>                                  # insurance fund vault
```

### Kamino
```
KLend2g3cZ87EoGDubt5QCWkPZABLBLiuqGkUQUsqo1   # program
<lending_market>                                  # lending market
<usdc_reserve>                                    # USDC reserve
<scope_oracle_accounts>                           # oracle feeds
```

### Jupiter LP
```
PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu   # program
5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq   # JLP pool
27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4   # JLP mint
<all_5_oracle_accounts>                           # price feeds
<all_5_custody_accounts>                          # custodies
```

### Maple
```
<maple_program>                                   # TBD
<usdc_pool>                                       # TBD
```

---

## Покроковий план реалізації

### Крок 1 — Surfpool setup (день 1–2)
- [ ] Встановити Surfpool: `cargo install surfpool`
- [ ] Отримати Helius/QuickNode API key для mainnet RPC
- [ ] Скрипт: `scripts/start-surfpool.sh` — запуск з правильними параметрами
- [ ] Верифікація: `surfpool` підіймається і клонує акаунти

### Крок 2 — Test helpers (день 2–3)
- [ ] `mintUsdc(wallet, amount)` — через Surfpool cheatcode
- [ ] `createATA(wallet, mint)` — Associated Token Account
- [ ] `getUsdcBalance(wallet)` — зручна обгортка
- [ ] `advanceClock(seconds)` — просуває `Clock.unix_timestamp` для cooldown тестів
- [ ] Налаштувати Mocha timeout: `timeout: 120_000` (2 хв) — fork тести повільні

```typescript
// helpers.ts — актуальні Surfpool cheatcode назви потрібно верифікувати
// через `surfpool --help` або документацію після встановлення.
// Приблизні назви (можуть відрізнятись у вашій версії Surfpool):

export async function mintUsdc(
  connection: Connection,
  recipient: PublicKey,
  amount: bigint
): Promise<void> {
  await connection.request("surfpool_setTokenAccountBalance", [
    recipient.toBase58(),
    USDC_MINT.toBase58(),
    amount.toString(),
  ]);
}

export async function advanceClock(
  connection: Connection,
  seconds: number
): Promise<void> {
  // ВАЖЛИВО: для time-based cooldowns (Drift ~13 днів, Maple withdrawal window)
  // потрібно просувати саме Clock.unix_timestamp, а не просто слоти.
  // Slot advancement ≠ time advancement (slot time ~400ms але не гарантовано).
  await connection.request("surfpool_advanceClock", [seconds]);
  // Якщо такого cheatcode немає — альтернатива: warp_to_timestamp через
  // solana-program-test SetClock syscall (тільки для LiteSVM unit тестів).
}
```

> **Верифікувати перед використанням:** `NO_DNA=1 surfpool --help` або Surfpool GitHub README для актуальних назв cheatcode методів.

### Крок 3 — MarginFi тест (день 3–4)

```typescript
describe("MarginFi Adapter", () => {
  const depositAmount = 100_000_000n; // 100 USDC

  it("deposits USDC and receives shares", async () => {
    await mintUsdc(connection, user.publicKey, depositAmount); // connection, не provider
    
    const tx = await dispatcherProgram.methods
      .deposit(new BN(depositAmount.toString()))
      .accounts({ adapter: MARGINFI_ADAPTER, ...accounts })
      .rpc();
    
    const position = await getUserPosition(user.publicKey, MARGINFI_ADAPTER);
    assert(position.shares > 0n, "shares should be > 0");
  });

  it("current_value >= deposited amount", async () => {
    // current_value через simulateTransaction → returnData
    const sim = await connection.simulateTransaction(
      await buildCurrentValueTx(dispatcherProgram, user, MARGINFI_ADAPTER),
      { replaceRecentBlockhash: true }
    );
    const valueBytes = Buffer.from(sim.value.returnData!.data[0], "base64");
    const value = valueBytes.readBigUInt64LE(0);
    assert(value >= depositAmount, "value should not decrease");
  });

  it("withdraws and returns USDC", async () => {
    const balanceBefore = await getUsdcBalance(user.publicKey);
    
    await dispatcherProgram.methods
      .withdraw(new BN(depositAmount.toString()))
      .accounts({ adapter: MARGINFI_ADAPTER, ...accounts })
      .rpc();
    
    const balanceAfter = await getUsdcBalance(user.publicKey);
    assert(balanceAfter >= balanceBefore + depositAmount * 99n / 100n,
      "should receive ≥99% back");
  });
});
```

### Крок 4 — Drift тест (день 4–5)

```typescript
describe("Drift Insurance Fund Adapter", () => {
  it("stakes USDC", async () => { ... });
  
  it("requests unstake (initiates cooldown)", async () => {
    const tx = await dispatcherProgram.methods.withdraw(new BN(shares))...;
    const position = await getUserPosition(...);
    assert(position.pendingWithdrawal, "pending_withdrawal should be true");
  });

  it("withdraw before cooldown returns CooldownActive error", async () => {
    // Anchor error format: { code: "CooldownActive", msg: "...", number: 6101 }
    // Regex на message не стабільний — перевіряємо числовий код
    await assert.rejects(
      dispatcherProgram.methods.withdraw(new BN(0)).rpc(),
      (err: AnchorError) => {
        assert.strictEqual(err.error.errorCode.number, 6101); // CooldownActive
        return true;
      }
    );
  });
  
  it("completes unstake after cooldown (~13 days)", async () => {
    // Просуваємо Clock.unix_timestamp, не просто слоти
    await advanceClock(14 * 24 * 3600); // 14 днів з запасом
    
    const tx = await dispatcherProgram.methods.withdraw(new BN(0)).rpc();
    const balance = await getUsdcBalance(user.publicKey);
    assert(balance > 0n, "should receive USDC back");
  });
});
```

### Крок 5 — Kamino тест (день 5–6)
- [ ] Seed USDC Reserve та oracle акаунти
- [ ] Тест deposit → kTokens
- [ ] Simulate time (refresh accumulates interest)
- [ ] Тест current_value > deposited

### Крок 6 — Jupiter LP тест (день 6–7)
- [ ] Seed JLP Pool + всі 5 custody oracle акаунтів
- [ ] Тест addLiquidity → JLP tokens
- [ ] Тест current_value в USD
- [ ] Тест removeLiquidity → USDC

### Крок 7 — Maple тест (день 7–8)
- [ ] Seed Maple pool акаунти
- [ ] Тест deposit → pool shares
- [ ] Тест withdrawal window (mock якщо > 7 днів)

### Крок 8 — Integration тести через Dispatcher (день 8–9)

**Happy path:**
- [ ] Повний flow: Dispatcher → MarginFi Adapter → MarginFi Protocol
- [ ] Тест перемикання між адаптерами (deposit в один, withdraw з іншого — повинен fail)

**Negative tests (обов'язково):**
- [ ] Незареєстрований адаптер → error code 6200 (`AdapterNotRegistered`)
- [ ] Revoked адаптер → error code 6201 (`AdapterRevoked`)
- [ ] Paused адаптер → error code 6202 (`AdapterPaused`)
- [ ] Deposit більше ніж баланс → token program error (insufficient funds)
- [ ] Withdraw більше ніж shares → error code 6001 (`InsufficientShares`)
- [ ] Withdraw при `pending_withdrawal = true` без достатнього cooldown → error 6101 (`CooldownActive`)
- [ ] Повторний `initialize_position` → Anchor `already in use` (PDA exists)
- [ ] Wrong `user_position` owner → `ConstraintSeeds` або `ConstraintOwner`
- [ ] Перевіряти числові error коди, не regex по message (стабільніше)

### Крок 9 — CI конфігурація (день 9–10)
- [ ] GitHub Actions workflow для запуску тестів
- [ ] Surfpool запускається in background перед тестами: `NO_DNA=1 surfpool start &`
- [ ] Secrets: `HELIUS_API_KEY` у GitHub Secrets
- [ ] Кешування Surfpool account snapshots (щоб не клонувати mainnet при кожному run)
- [ ] `.mocharc.yml` з `timeout: 120000` і `exit: true`
- [ ] Розділити unit тести (LiteSVM, швидкі) і fork тести (Surfpool, повільні) в окремі npm scripts

---

## Surfpool Cheatcodes

```typescript
// Встановити баланс токен акаунта
await connection.request("surfpool_setTokenAccountBalance", [...]);

// Просунути Clock.unix_timestamp (для cooldown тестів — НЕ advanceSlots)
await connection.request("surfpool_advanceClock", [seconds]);

// Клонувати акаунт з mainnet в runtime
await connection.request("surfpool_cloneAccount", [address]);

// ВАЖЛИВО: назви методів верифікувати через `NO_DNA=1 surfpool --help`
// surfpool_advanceSlots і surfpool_advanceClock — різні речі:
// - advanceSlots: просуває slot counter (не змінює unix_timestamp пропорційно)
// - advanceClock: змінює Clock.unix_timestamp безпосередньо (потрібно для cooldown)
```

---

## Структура акаунтів для клонування (accounts.ts)

```typescript
export const MAINNET_ACCOUNTS = {
  marginfi: {
    program: "MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA",
    group: "4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG",
    usdcBank: "2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB",
  },
  drift: {
    program: "dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH",
    state: "FxztXFc4aiyHzGHCTzJRys5X4TnWVBzJTYeSMn7b65nA",
  },
  kamino: {
    program: "KLend2g3cZ87EoGDubt5QCWkPZABLBLiuqGkUQUsqo1",
  },
  jupiterLp: {
    program: "PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu",
    pool: "5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq",
    jlpMint: "27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4",
  },
  usdc: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
} as const;
```

---

## Ризики

| Ризик | Мітигація |
|---|---|
| Mainnet акаунти змінились після fork | Snapshot акаунти при першому запуску |
| Surfpool нестабільний | Pin версію, мати fallback на solana-test-validator |
| Тести флакі через oracle staleness | Mock oracle у fork через cheatcodes |
| RPC rate limiting | Локальний кеш акаунтів, батчинг fetch |

---

## Залежності

- Залежить від: всіх 5 адаптерів (мають бути скомпільовані)
- Залежить від: Dispatcher та Registry (мають бути deployed)
- Потребує: Helius/QuickNode API key

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| Surfpool setup + helpers | 2 дні |
| MarginFi тести | 1 день |
| Drift тести (cooldown) | 2 дні |
| Kamino тести | 1 день |
| Jupiter LP тести (oracle seeding) | 2 дні |
| Maple тести | 1 день |
| Integration тести (Dispatcher) | 2 дні |
| CI конфігурація | 1 день |
| **Разом** | **~12 днів** |

---

## Definition of Done

- [ ] Всі 5 адаптерів мають passing mainnet-fork тести
- [ ] `deposit` → `current_value` → `withdraw` happy path проходить для кожного
- [ ] Негативні тести: незареєстрований адаптер, revoked, paused, insufficient funds/shares
- [ ] Cooldown/window поведінка протестована через `advanceClock`
- [ ] CI workflow запускає тести автоматично
- [ ] `.mocharc.yml` з `timeout: 120000`
- [ ] README з інструкціями як запустити тести локально (включаючи env vars)
