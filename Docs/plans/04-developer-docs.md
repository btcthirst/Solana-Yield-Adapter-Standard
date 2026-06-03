# План: Developer Documentation

## Мета

Два документи для розробників:
1. **Adapter Standard Specification** — формальна специфікація інтерфейсу
2. **"Build Your Own Adapter" Guide** — покроковий туторіал

Критерій успіху: новий розробник має зробити робочий адаптер **за менше ніж 1 день**.

---

## Документ 1: Adapter Standard Specification

**Файл:** `SPEC.md` у корені репозиторію

### Структура

```markdown
# Solana Yield Adapter Standard v1.0

## Abstract
## Motivation
## Terminology
## Interface Specification
  ### deposit(amount: u64) → Result<()>
  ### withdraw(shares: u64) → Result<()>
  ### current_value() → Result<()>
## Account Conventions
  ### Required PDAs
  ### Naming Conventions
## Error Codes
## Event Schema
## Registry Integration
## Security Considerations
## Reference Implementations
## Changelog
```

### Зміст Interface Specification

```rust
/// Adapter Convention: кожен адаптер ПОВИНЕН мати ці три інструкції.
/// Solana інструкції повертають Result<()> — значення записуються в акаунти.

/// Депозит активів у протокол.
/// @param amount   Кількість базового токена (6 decimals для USDC)
/// POST: ctx.accounts.user_position.shares збільшено на отримані shares
/// @errors InsufficientFunds, ProtocolError, SlippageExceeded
deposit(ctx: Context<Deposit>, amount: u64) -> Result<()>

/// Виведення активів з протоколу.
/// @param shares   Кількість shares для виведення (0 = withdraw all)
/// POST: ctx.accounts.user_position.shares зменшено
/// @errors InsufficientShares, CooldownActive, ProtocolError
withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()>

/// Поточна вартість позиції у базовому токені.
/// POST: sol_set_return_data(&value_u64.to_le_bytes())
/// @note  Викликати через simulateTransaction — не потребує fees
/// @note  Може бути stale якщо протокол не оновлював стан
current_value(ctx: Context<CurrentValue>) -> Result<()>
```

> **Чому Result<()>:** Solana VM не підтримує return values з інструкцій. Клієнт читає результат через зміни в акаунтах після транзакції або через simulateTransaction.

### Account Conventions

```
PDA Seeds (обов'язково):
- UserPosition (Dispatcher):   ["position", user_pubkey, adapter_pubkey]
- AdapterPosition (Adapter):   ["<protocol>_position", user_pubkey]  // protocol-specific state

Token Flow (два варіанти залежно від протоколу):
- Варіант A (MarginFi, Kamino, Drift):
    deposit:  user_ata → protocol vault (напряму через CPI, адаптер = signer)
    withdraw: protocol vault → user_ata (напряму через CPI)
- Варіант B (Jupiter LP та ін. де адаптер холдить токени):
    deposit:  user_ata → protocol vault; protocol → adapter_jlp_ata (JLP до адаптера)
    withdraw: adapter_jlp_ata → protocol vault → user_ata

Naming:
- Base asset = "asset" (не "liquidity", не "token")
- Protocol shares = "shares" у UserPosition
- Adapter-specific state = окремий AdapterPosition PDA (не в UserPosition)
- Exchange rate = "share_price" або "exchange_rate"

AccountInfo rules:
- Кожен AccountInfo потребує /// CHECK: коментар (Anchor вимагає)
- Мутабельні акаунти: тільки якщо протокол їх пише (не `mut` за замовчуванням)
```

### Error Code Registry

```rust
pub enum AdapterError {
    // 6000 - загальні
    InsufficientFunds = 6000,
    InsufficientShares = 6001,
    SlippageExceeded = 6002,
    Overflow = 6003,               // u128→u64 conversion failed

    // 6100 - протокол-специфічні
    ProtocolError = 6100,
    CooldownActive = 6101,         // Drift, Maple — cooldown не пройшов
    WithdrawalWindowClosed = 6102, // Maple
    OracleStale = 6103,

    // 6200 - registry
    AdapterNotRegistered = 6200,
    AdapterRevoked = 6201,
    AdapterPaused = 6202,          // is_paused = true у RegistryEntry
}
```

---

## Документ 2: "Build Your Own Adapter" Guide

**Файл:** `ADAPTER_GUIDE.md` у корені репозиторію

### Структура (< 1 день туторіал)

```
ADAPTER_GUIDE.md

Part 1: Prerequisites (15 хв)
Part 2: Scaffold Your Adapter (30 хв)
Part 3: Implement deposit() (2 год)
Part 4: Implement withdraw() (1 год)
Part 5: Implement current_value() (1 год)
Part 6: Write Tests (2 год)
Part 7: Register in Registry (30 хв)
Total: ~7-8 годин
```

### Зміст кожної частини

#### Part 1: Prerequisites
```markdown
- Rust 1.79+, Anchor 0.31.1, Solana CLI 2.2.20
- Базове розуміння: PDAs, CPIs, Anchor macros
- Протокол що ви хочете інтегрувати має:
  □ Публічний IDL або відомий ABI
  □ USDC або інший стабільний asset
  □ deposit/withdraw інструкції
```

#### Part 2: Scaffold

```bash
# Копіювати template
cp -r programs/adapters/marginfi programs/adapters/my-protocol

# Оновити Cargo.toml
# Оновити lib.rs program ID
anchor keys generate
anchor build
```

Надати `adapter-template/` у репозиторії з:
- `lib.rs` з placeholder інструкціями
- `state.rs` з `UserPosition` struct
- `error.rs` зі стандартними error codes
- тестовий файл з TODO коментарями

#### Part 3: deposit() — детальний walkthrough

```rust
// Покроковий код з поясненнями:

// 1. Валідувати amount > 0
require!(amount > 0, AdapterError::InsufficientFunds);

// 2a. Якщо протокол приймає токени напряму (MarginFi, Kamino, Drift):
//     Адаптер підписує як PDA-власник акаунту у протоколі — ніякого adapter_vault
your_protocol::cpi::deposit(
    CpiContext::new_with_signer(
        ctx.accounts.protocol_program.to_account_info(),
        YourProtocolDeposit {
            signer: ctx.accounts.adapter_authority.to_account_info(), // PDA
            user_token_account: ctx.accounts.user_ata.to_account_info(),
            // ... інші акаунти
        },
        &[&[b"adapter_authority", &[ctx.accounts.adapter_state.bump]]],
    ),
    amount,
)?;

// 2b. Якщо протокол повертає share-токени (Jupiter LP):
//     Адаптер холдить share-токени від імені user
//     Попередньо: user transfer USDC → adapter_usdc_vault, потім CPI

// 3. Записати shares у UserPosition (читати з протоколу після CPI)
let shares_received = read_shares_from_protocol(&ctx)?;
ctx.accounts.user_position.shares = ctx.accounts.user_position.shares
    .checked_add(shares_received)
    .ok_or(error!(AdapterError::Overflow))?;

// ВАЖЛИВО: ComputeBudget — клієнтська інструкція, не CPI:
// tx.add(ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }))
```

#### Part 4: withdraw() — cooldown handling

```markdown
Деякі протоколи мають cooldown. Є два патерни:

**Immediate withdraw** (MarginFi, Kamino):
withdraw() → CPI → токени повернуто одразу

**Two-step withdraw** (Drift, Maple):
withdraw() → CPI request → [cooldown] → withdraw() знову → токени
Detect via: check user_position.pending_withdrawal flag
```

#### Part 5: current_value() — oracle та math

```rust
// Три підходи — ЗАВЖДИ порядок: mul-before-div для точності

// A: Exchange rate (MarginFi, Kamino)
// user_shares * pool.total_assets / pool.total_shares  ← правильний порядок!
// (не total_assets / total_shares * user_shares — precision loss)

// B: On-chain AUM (Jupiter LP)
// user_jlp * pool.aum_usd / jlp_total_supply

// C: Direct vault accounting (Drift IF)
// user_if_shares * vault_balance / total_if_shares

// Шаблон для всіх варіантів:
let value = (user_shares as u128)
    .checked_mul(numerator as u128)         // спочатку множення
    .and_then(|v| v.checked_div(denominator as u128))
    .and_then(|v| u64::try_from(v).ok())    // НЕ "as u64" — panic при overflow
    .ok_or(error!(AdapterError::Overflow))?;

set_return_data(&value.to_le_bytes());      // повертати через return_data
```

#### Part 6: Tests

```typescript
// Мінімальний тест файл
describe("My Protocol Adapter", () => {
  it("full lifecycle: deposit → current_value → withdraw", async () => {
    // 1. Mint test USDC
    await mintUsdc(connection, user.publicKey, 100_000_000n);
    
    // 2. Deposit
    await program.methods.deposit(new BN(100_000_000)).rpc();
    const position = await getPosition(user.publicKey);
    assert(position.shares > 0, "shares must be > 0 after deposit");
    
    // 3. Check value via simulateTransaction → returnData
    const sim = await connection.simulateTransaction(
      await program.methods.currentValue().transaction(),
      { replaceRecentBlockhash: true }
    );
    const valueBytes = Buffer.from(sim.value.returnData!.data[0], "base64");
    const value = valueBytes.readBigUInt64LE(0);
    assert(Number(value) >= 99_000_000, "value ≥ 99 USDC");

    // 4. Withdraw — position.shares є BN/bigint, потрібен явний тип
    await program.methods.withdraw(new BN(position.shares.toString())).rpc();
    const balance = await getUsdcBalance(user.publicKey);
    assert(balance >= 99_000_000n, "received ≥ 99 USDC");
  });
});
```

#### Part 7: Register in Registry

```bash
# Задеплоїти адаптер
NO_DNA=1 anchor deploy --provider.cluster devnet

# Зареєструвати через CLI скрипт
# (скрипт знаходиться у scripts/register-adapter.ts репозиторію)
ts-node scripts/register-adapter.ts \
  --adapter-program-id <YOUR_ADAPTER_PROGRAM_ID> \
  --name "My Protocol USDC" \
  --protocol "My Protocol" \
  --asset-mint EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v \
  --registry <REGISTRY_PROGRAM_ID> \
  --keypair ~/.config/solana/id.json
```

---

## Покроковий план написання документів

### Крок 1 — Скласти SPEC.md (день 1–3)
- [ ] Написати Interface Specification після стабілізації інтерфейсу
- [ ] Задокументувати всі error codes
- [ ] Задокументувати event схеми
- [ ] Security considerations (reentrancy, oracle manipulation, PDA collisions)
- [ ] Review з точки зору: "чи зрозуміло новому dev?"

### Крок 2 — Створити adapter-template та register-adapter.ts (день 3–4)
- [ ] `programs/adapters/template/` — чистий scaffold з TODO
- [ ] Мінімальний `lib.rs` що компілюється (з трьома stub-інструкціями)
- [ ] `state.rs` з `UserPosition` struct і правильним `space` розрахунком
- [ ] Тестовий файл `tests/my-adapter.test.ts` з TODO
- [ ] `README.md` у template папці
- [ ] **`scripts/register-adapter.ts`** — TypeScript CLI для реєстрації адаптера у Registry:
  - Зчитує keypair і cluster з аргументів
  - Викликає `approve_adapter` на Registry program
  - Виводить підтвердження з посиланням на explorer

### Крок 3 — Написати ADAPTER_GUIDE.md (день 4–7)
- [ ] Part 1-2: Setup та Scaffold (швидко)
- [ ] Part 3: deposit() з детальним code walkthrough
- [ ] Part 4: withdraw() з cooldown patterns
- [ ] Part 5: current_value() з трьома підходами
- [ ] Part 6: Tests
- [ ] Part 7: Registration
- [ ] Фінальний review: "чи можна зробити за 1 день?"

### Крок 4 — Технічний review (день 7–8)
- [ ] Пройти через ADAPTER_GUIDE.md від нуля (dry-run)
- [ ] Перевірити всі команди що вони працюють
- [ ] Перевірити всі посилання між документами
- [ ] Граматичний review

---

## Файлова структура документів

```
/ (корінь репозиторію)
├── SPEC.md                    — Adapter Standard Specification
├── ADAPTER_GUIDE.md           — Build Your Own Adapter Guide
├── README.md                  — Огляд проекту + quick start
└── programs/
    └── adapters/
        └── template/          — Scaffold для нового адаптера
            ├── src/
            │   ├── lib.rs
            │   ├── state.rs
            │   └── error.rs
            ├── tests/
            │   └── my-adapter.test.ts
            └── README.md
```

---

## Залежності

- Залежить від: стабілізованого Core Dispatcher інтерфейсу
- Залежить від: хоча б 2-3 реалізованих адаптерів (для прикладів)
- **Писати паралельно** з останніми адаптерами, не після всіх

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| SPEC.md | 2 дні |
| adapter-template | 1 день |
| ADAPTER_GUIDE.md | 3 дні |
| Review + corrections | 2 дні |
| **Разом** | **~8 днів** |

---

## Definition of Done

- [ ] SPEC.md: всі три інструкції задокументовані з сигнатурами `Result<()>`, POST-умовами та errors
- [ ] SPEC.md: account conventions (UserPosition space, ValueResult), event schema, security considerations
- [ ] SPEC.md: пояснення чому `Result<()>` а не `Result<u64>` (Solana VM обмеження)
- [ ] ADAPTER_GUIDE.md: розробник може пройти від нуля до working adapter
- [ ] `programs/adapters/template/` компілюється без warnings
- [ ] `scripts/register-adapter.ts` працює на devnet
- [ ] Всі команди у гайді перевірені та працюють
- [ ] Час прочитання + виконання ADAPTER_GUIDE.md ≤ 8 годин
