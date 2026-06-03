# План: Maple Finance (Maple Syrup) Adapter

## Складність: ★★★★★ (Найскладніший)

## Мета

Адаптер для Maple Finance permissioned lending pools на Solana. Найменш задокументований протокол у списку.

---

## Як працює Maple Finance (контекст)

```
User USDC
    │  requestRedeem (permissioned)
    ▼
Maple Pool (institutional lending)
    │  виданий корпоративним позичальникам
    ▼
Pool Shares (syrup tokens або pool tokens)
    │  нараховує yield від loan interest
    ▼
User USDC + yield (через withdrawal cycle)
```

Ключові концепти:
- **Pool** — конкретний lending pool для певного позичальника
- **Pool Shares** — частки у pool, обліковуються on-chain
- **Withdrawal Window** — Maple має дискретні withdrawal periods (не миттєво)
- **Permissioned Deposits** — деякі pools вимагають whitelist
- **Maple Syrup** = загальна назва для Solana-версії (раніше був на Ethereum)

---

## КРИТИЧНЕ ЗАСТЕРЕЖЕННЯ

Maple Finance на Solana має **обмежену публічну документацію**. Перед початком реалізації:
1. Перевірити чи існує Maple на Solana взагалі активно (можливо перейшли на Ethereum Base)
2. Перевірити чи є публічний IDL
3. Якщо Maple недоступний — запасний план: замінити на **Solend** або **Marginfi другий pool**

---

## Дослідження (найперший крок)

### Крок 0 — Верифікація існування (день 1, БЛОКЕР)
- [ ] Перевірити: `solana program show <maple_program_id>` на mainnet
- [ ] Знайти Maple Solana program address (пошук у explorer.solana.com)
- [ ] Перевірити Twitter/Discord Maple Finance на статус Solana deployment
- [ ] Спробувати: `anchor idl fetch <maple_program_id>`
- [ ] Якщо IDL недоступний → перейти до реверс-інжинірингу

**Якщо Maple недоступний на Solana:** Замінити на **Solend USDC** (адреса: `So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo`)

---

## Необхідні зовнішні залежності

```toml
# Cargo.toml — немає офіційного crate
# Потрібно вручну описати structs з IDL або реверс-інжинірингу
```

```json
// package.json — перевірити наявність:
"@maplelabs/syrup-sdk": "latest"  // може не існувати для Solana
```

---

## Mainnet акаунти (потрібно знайти)

| Акаунт | Адреса | Статус |
|---|---|---|
| Maple Program | ? | Потрібно знайти |
| USDC Pool | ? | Потрібно знайти |
| Pool Token Mint | ? | Потрібно знайти |

**Джерела для пошуку:**
- https://maple.finance/docs
- https://explorer.solana.com (пошук "Maple")
- Maple Discord / GitHub

---

## Покроковий план реалізації (якщо IDL знайдено)

### Крок 1 — Дослідження (день 1–3)
- [ ] Знайти program ID на mainnet
- [ ] Завантажити IDL (або реконструювати)
- [ ] Знайти `deposit` / `withdraw` інструкції
- [ ] Визначити withdrawal window механізм
- [ ] Перевірити permissioning (чи потрібен whitelist)

### Крок 2 — Scaffold (день 3)
- [ ] `programs/adapters/maple/`
- [ ] Визначити мінімальні structs для Pool та Position

### Крок 3 — Permissioning дослідження (день 3–4)

Maple може вимагати:
```rust
// Варіант A: Відкритий pool (немає перевірки)
// Варіант B: Whitelist PDA для адаптера
// Варіант C: KYC через off-chain attestation
```

Якщо whitelist потрібен — зв'язатись з Maple team для whitelisting адаптера.

### Крок 4 — Deposit інструкція (день 4–6)
- [ ] Визначити всі необхідні акаунти
- [ ] Якщо є permissioning — реалізувати whitelist check
- [ ] CPI виклик deposit/contribute
- [ ] Зберегти pool shares у `UserPosition`

### Крок 5 — Withdraw з Window механізмом (день 6–9)

Maple withdrawal cycle:
```
requestWithdraw() → очікування window → executeWithdraw()
```

Аналогічно до Drift cooldown:
```rust
// withdraw() = requestWithdraw, якщо window не активне
// withdraw() = executeWithdraw, якщо window активне
// current_value() включає pending withdrawal
```

- [ ] Створити `MapleAdapterPosition` PDA для Maple-specific state:
  ```rust
  #[account]
  pub struct MapleAdapterPosition {
      pub pending_withdrawal: bool,
      pub withdrawal_request_timestamp: i64,
      pub withdrawal_request_shares: u64,
      pub bump: u8,
  }
  // seeds: [b"maple_position", user.key]
  // НЕ зберігати в generic UserPosition — це adapter-specific state
  ```
- [ ] Emit `WithdrawalWindowOpen` event з `expected_ready_at` timestamp
- [ ] Документувати затримку у spec

### Крок 6 — current_value (день 9)

```rust
// pool_shares * pool_nav_per_share = USDC value
// NAV per share зберігається у Pool акаунті
pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
    let pool_data = ctx.accounts.pool.try_borrow_data()?; // безпечно
    let pool = Pool::try_deserialize(&mut &pool_data[..])?;
    let position = &ctx.accounts.user_position;

    // Перевірити точні decimals nav_per_share через IDL
    // Приклад: якщо nav_per_share має 6 decimals
    let value = (position.shares as u128)
        .checked_mul(pool.nav_per_share as u128)
        .and_then(|v| v.checked_div(1_000_000u128))
        .and_then(|v| u64::try_from(v).ok()) // безпечний cast замість as u64
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
```

### Крок 7 — Unit тести (день 9–11)
- [ ] Мінімальні тести через LiteSVM
- [ ] Тест: deposit → shares > 0
- [ ] Тест: current_value коректний
- [ ] Тест: withdrawal window поведінка

---

## Запасний план: Solend USDC Adapter

Якщо Maple Finance недоступний або має критичні блокери:

```
Solend Program: So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo
USDC Reserve: знайти через Solend SDK
Складність: ★★★☆☆ (аналогічна MarginFi)
```

Solend має:
- Відкритий IDL
- Хорошу документацію
- Активний mainnet deployment
- SDK: `@solendprotocol/solend-sdk`

**Рекомендація:** Запропонувати Maple але мати Solend як fallback. Зазначити у submission що Maple замінений якщо потрібно.

---

## Реверс-інжиніринг IDL (якщо немає публічного)

```bash
# 1. Завантажити програму bytecode
solana program dump <program_id> maple.so

# 2. Знайти discriminators (перші 8 байт кожної інструкції)
# Вони детерміновані: sha256("global:<instruction_name>")[0..8]

# 3. Реконструювати account structs з discriminator + field layout
# Використати tools: https://github.com/acheronfail/solana-ape або Shank

# 4. Написати мінімальні Rust structs вручну
```

---

## Ризики

| Ризик | Ймовірність | Вирішення |
|---|---|---|
| Maple не активний на Solana | Висока | Замінити на Solend |
| Permissioned deposit (whitelist) | Висока | Контакт з Maple team |
| Немає публічного IDL | Висока | Реверс-інжиніринг або заміна |
| Withdrawal window > 7 днів | Середня | Документувати, тест мокає window |

---

## Залежності

- Залежить від: Core Dispatcher (заморожений)
- Найбільш незалежний від інших адаптерів
- **Дослідницька фаза має бути ПЕРШОЮ** — може потрібна заміна протоколу

---

## Оцінка часу

| Завдання | Часу (якщо IDL доступний) | Часу (реверс-інжиніринг) |
|---|---|---|
| Дослідження та верифікація | 3 дні | 5 днів |
| Deposit | 2 дні | 3 дні |
| Withdraw (window) | 3 дні | 4 дні |
| current_value | 1 день | 2 дні |
| Unit тести | 2 дні | 2 дні |
| **Разом** | **~11 днів** | **~16 днів** |

---

## Definition of Done

- [ ] Maple або Solend (fallback) адаптер реалізований
- [ ] Withdrawal window / cooldown коректно обробляється
- [ ] Mainnet-fork тест проходить
- [ ] Якщо використаний fallback — зазначено у документації з поясненням
