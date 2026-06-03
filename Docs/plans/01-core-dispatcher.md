# План: Core Dispatcher Contract

## Мета

Anchor-програма, що виступає єдиною точкою входу для всіх операцій з yield-адаптерами. Реалізує стандартний інтерфейс і делегує виконання конкретному адаптеру через CPI.

---

## Стандартний інтерфейс

```rust
// Три обов'язкові інструкції для кожного адаптера.
// Solana інструкції завжди повертають Result<()> —
// результати (shares, value) записуються в акаунти, клієнт читає їх після tx.
pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()>
    // → пише shares до ctx.accounts.user_position.shares

pub fn withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()>
    // → оновлює ctx.accounts.user_position.shares (зменшує або до 0)
    // → shares = 0 означає "withdraw all" + закриває UserPosition PDA (повертає rent)

pub fn current_value(ctx: Context<CurrentValue>) -> Result<()>
    // → sol_set_return_data(&value_u64.to_le_bytes())
    // Клієнт викликає через simulateTransaction → returnData (не платить fees)
```

> **Важливо:** Solana не підтримує повернення значень з інструкцій. `deposit`/`withdraw` оновлюють `user_position.shares` в акаунті. `current_value` повертає значення через `set_return_data` — клієнт читає через `simulateTransaction → returnData`.

Dispatcher не містить бізнес-логіки — він:
1. Валідує що адаптер зареєстрований у Registry
2. Встановлює Compute Unit limit (`ComputeBudgetProgram`) для CPI-ланцюжка
3. Формує правильний набір акаунтів для CPI
4. Викликає відповідну інструкцію адаптера

---

## Архітектура

```
User
  │
  ▼
Dispatcher Program (router)
  │  перевіряє Registry
  │  формує CPI accounts
  ▼
Adapter Program (конкретний протокол)
  │  CPI
  ▼
Protocol Program (Kamino / MarginFi / etc)
```

### Ключові акаунти Dispatcher

```rust
// AdapterConfig ВИДАЛЕНО — дублює RegistryEntry.
// Dispatcher читає RegistryEntry напряму з Registry Program.

#[account]
pub struct UserPosition {
    pub owner: Pubkey,
    pub adapter: Pubkey,
    pub shares: u64,   // shares у протоколі (source of truth)
    pub bump: u8,
}
// space = 8 + 32 + 32 + 8 + 1 = 81 bytes
//
// Примітка: deposited_amount видалено — shares є єдиним source of truth.
// Adapter-specific state (pending_withdrawal, cooldown timestamp тощо)
// зберігається в окремому AdapterPosition PDA всередині кожного адаптера,
// а не тут — щоб не робити UserPosition залежним від протоколу.
```

> **`current_value` без окремого акаунту:** Замість `ValueResult` акаунту використовується `sol_set_return_data` (Solana 1.14+). Адаптер записує value через `set_return_data(&value.to_le_bytes())`, клієнт читає через `simulateTransaction` → `returnData`. Це усуває потребу в ініціалізації/rent для ephemeral акаунту.

---

## Покроковий план реалізації

### Крок 1 — Scaffold проекту (день 1)
- [ ] `anchor init yield-adapter-standard`
- [ ] Налаштувати `Anchor.toml` з правильними feature flags
- [ ] Додати workspace members: `dispatcher`, `registry`, адаптери як окремі crates
- [ ] Зафіксувати версії: Anchor 0.31.1, Solana 2.2.20

### Крок 2 — Визначити shared типи (день 1–2)
- [ ] Створити `programs/adapter-interface/` — shared crate з конвенцією
- [ ] Визначити **Adapter Convention** (не Rust trait — Anchor інструкції не можуть реалізувати trait):
  ```rust
  // Це документаційна конвенція, не impl-able trait.
  // Кожен адаптер ПОВИНЕН мати ці три інструкції з такими сигнатурами:
  //
  // deposit(ctx: Context<Deposit>, amount: u64) -> Result<()>
  //   POST: ctx.accounts.user_position.shares збільшено на отримані shares
  //
  // withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()>
  //   PRE:  shares <= ctx.accounts.user_position.shares (або 0 = all)
  //   POST: ctx.accounts.user_position.shares зменшено
  //
  // current_value(ctx: Context<CurrentValue>) -> Result<()>
  //   POST: sol_set_return_data(&value_u64.to_le_bytes())
  //         Клієнт читає через simulateTransaction → returnData.data
  //
  // Також обов'язково: initialize_position(adapter: Pubkey) -> Result<()>
  //   Створює UserPosition PDA перед першим deposit
  ```
- [ ] Визначити стандартні error codes (shared enum)
- [ ] Визначити event structs для logging

### Крок 3 — Dispatcher інструкції (день 2–4)
- [ ] `initialize_dispatcher` — ініціалізація dispatcher state
- [ ] `initialize_position(adapter: Pubkey)` — створює UserPosition PDA для user/adapter пари
- [ ] `deposit(amount: u64)` — роутинг до адаптера, оновлює `UserPosition.shares`
- [ ] `withdraw(shares: u64)` — роутинг до адаптера, `shares=0` = повне виведення та закриття `UserPosition` PDA (повертає rent до `owner`)
- [ ] `current_value` — роутинг до адаптера, читає return_data після CPI

> **ComputeBudget:** `set_compute_unit_limit` — це НЕ CPI зсередині програми. Це окрема інструкція яку **клієнт** додає першою в транзакцію:
> ```typescript
> const tx = new Transaction()
>   .add(ComputeBudgetProgram.setComputeUnitLimit({ units: 500_000 }))
>   .add(dispatcherIx);
> ```
> Програма не може змінити свій CU limit через CPI.

### Крок 4 — Account validation (день 4–5)
- [ ] PDA derivation для `UserPosition`: `[b"position", user.key, adapter.key]`
- [ ] Dispatcher читає `RegistryEntry` PDA з Registry (не `AdapterConfig` — його немає)
- [ ] Перевірка: `registry_entry.is_active && !registry_entry.is_paused`
- [ ] Перевірка: `registry_entry.adapter_program_id == adapter_program.key()`

### Крок 5 — CPI механізм (день 5–7)
- [ ] Generic CPI builder що приймає `remaining_accounts`
- [ ] Серіалізація discriminator + args для виклику адаптера
- [ ] Читання `return_data` після CPI для `current_value`: `get_return_data()`

### Крок 6 — Unit тести (день 7–10)
- [ ] LiteSVM тести для кожної інструкції
- [ ] Тест: deposit → перевірити UserPosition оновлено
- [ ] Тест: withdraw → перевірити баланс повернуто
- [ ] Тест: незареєстрований адаптер → очікуємо error
- [ ] Тест: неправильний owner → очікуємо error

---

## Структура файлів

```
programs/
├── dispatcher/
│   ├── src/
│   │   ├── lib.rs              — точка входу, інструкції
│   │   ├── instructions/
│   │   │   ├── deposit.rs
│   │   │   ├── withdraw.rs
│   │   │   ├── current_value.rs
│   │   │   └── mod.rs
│   │   ├── state/
│   │   │   ├── user_position.rs
│   │   │   └── mod.rs
│   │   ├── error.rs
│   │   └── events.rs
│   └── Cargo.toml
└── adapter-interface/
    ├── src/
    │   ├── lib.rs
    │   ├── convention.rs   — документаційна конвенція (не impl-able trait)
    │   └── types.rs
    └── Cargo.toml
```

---

## Критичні рішення дизайну

### `remaining_accounts` vs фіксовані акаунти
Кожен протокол потребує різних акаунтів. Dispatcher передає їх через `remaining_accounts` — адаптер сам знає які взяти і в якому порядку. Це дозволяє уникнути змін у Dispatcher при додаванні нових адаптерів.

### `current_value` як view-функція
Solana не має view-функцій. Обраний підхід: **`sol_set_return_data` + `simulateTransaction`**.

```rust
// В адаптері (on-chain):
use solana_program::program::set_return_data;
let value: u64 = calculate_value(...)?;
set_return_data(&value.to_le_bytes());
```

```typescript
// Клієнт — не платить fees, не змінює стан:
const sim = await connection.simulateTransaction(tx, {
  commitment: "confirmed",
  replaceRecentBlockhash: true,
});
if (sim.value.returnData) {
  const bytes = Buffer.from(sim.value.returnData.data[0], "base64");
  const value = bytes.readBigUInt64LE(0);
}
```

Переваги над `ValueResult` акаунтом: не потрібно ініціалізувати акаунт, не блокується rent, простіший lifecycle.

### Share-based accounting
Dispatcher зберігає `shares`, а не `amount`. `current_value` конвертує shares → assets за поточним курсом протоколу.

---

## Залежності

- Не залежить від жодного адаптера
- **Dispatcher читає Registry** (не навпаки) для валідації кожного виклику
- **Публічний інтерфейс має бути заморожений до початку 3-го адаптера**

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| Scaffold + shared types | 2 дні |
| Dispatcher інструкції | 3 дні |
| CPI механізм | 2 дні |
| Unit тести | 3 дні |
| **Разом** | **~10 днів** |

---

## Definition of Done

- [ ] `initialize_position` / `deposit` / `withdraw` / `current_value` компілюються і мають тести
- [ ] CPI виклик адаптера працює з mock-адаптером
- [ ] `current_value` повертає значення через `set_return_data`, клієнт читає через `simulateTransaction`
- [ ] Незареєстрований адаптер повертає `AdapterNotRegistered` error
- [ ] Paused адаптер повертає `AdapterPaused` error
- [ ] `anchor build` проходить без warnings
- [ ] `initialize_position` → `deposit` → `withdraw` повний flow протестований
- [ ] `withdraw(shares=0)` закриває `UserPosition` і повертає rent
- [ ] ComputeBudget встановлюється на **клієнті**, не через CPI
- [ ] README з описом інтерфейсу та TypeScript прикладом для `current_value`
