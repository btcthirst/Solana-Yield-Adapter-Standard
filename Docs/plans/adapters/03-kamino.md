# План: Kamino USDC Adapter

## Складність: ★★★★☆

## Мета

Адаптер для Kamino Finance USDC через **Kamino Lending** (не Liquidity Vaults). Kamino Lending — це lending protocol поверх Orca/Raydium CLMM, аналогічний до MarginFi але з іншою архітектурою акаунтів.

---

## Як працює Kamino (контекст)

```
User USDC
    │  deposit (Kamino Lending)
    ▼
Kamino Lending Reserve (USDC)
    │  видає collateral token (cUSDC / kUSDC)
    ▼
kUSDC (collateral token, зберігається в Obligation адаптера)
    │  exchange rate зростає з нарахуванням interest
    ▼
User USDC + yield (при withdraw = redeem collateral → liquidity)
```

> **Уточнення:** Обрано Kamino Lending, а не Kamino Liquidity Vaults. Liquidity Vaults управляють CLMM позиціями — значно складніше. Lending простіше і має чіткіший IDL.

Ключові концепти (Kamino Lending):
- **Reserve** — пул ліквідності для конкретного токена (USDC Reserve)
- **Obligation** — позиція конкретного кредитора/позичальника
- **cToken / kToken** — collateral token, мінтується при deposit, спалюється при withdraw
- **Scope** — Kamino's oracle для pricing (не Pyth напряму)

---

## Необхідні зовнішні залежності

```toml
# Cargo.toml
# Rust crate для Kamino Lending (не TypeScript SDK):
klend = { git = "https://github.com/Kamino-Finance/klend", features = ["no-entrypoint"] }
# Перевірити актуальний repo і feature flags перед початком —
# Kamino могли перейменувати crate або змінити структуру.
```

```json
// package.json
"@kamino-finance/klend-sdk": "latest"
// kliquidity-sdk видалено — це для Kamino Liquidity Vaults, нам не потрібно
```

**IDL:** `anchor idl fetch KLend2g3cZ87EoGDubt5QCWkPZABLBLiuqGkUQUsqo1` (lending)

---

## Два типи Kamino USDC vaults

### Варіант A: Kamino Lending (простіший)
Kamino Lending — це CLMM-wrapped lending, схожий на MarginFi але з додатковим шаром.

### Варіант B: Kamino Liquidity Vaults (складніший)
Автоматичний LP management у CLMM пулах.

**Рекомендація:** реалізувати **Kamino Lending** (USDC lending vault) — менша складність, є чіткіший IDL.

---

## Mainnet акаунти (Kamino Lending USDC)

| Акаунт | Адреса |
|---|---|
| Kamino Lending Program | `KLend2g3cZ87EoGDubt5QCWkPZABLBLiuqGkUQUsqo1` |
| USDC Reserve | потрібно знайти через SDK/explorer |
| Lending Market | потрібно знайти через SDK |
| USDC Mint | `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` |

---

## Покроковий план реалізації

### Крок 1 — Дослідити Kamino IDL (день 1–2)
- [ ] Завантажити IDL: `anchor idl fetch KLend2g3cZ87EoGDubt5QCWkPZABLBLiuqGkUQUsqo1 -o kamino.json`
- [ ] Знайти `deposit_reserve_liquidity_and_obligation_collateral` інструкцію
- [ ] Знайти `withdraw_obligation_collateral_and_redeem_reserve_collateral` інструкцію
- [ ] Визначити всі 15+ акаунтів (Kamino має складні account lists)
- [ ] Вивчити SDK код для reference: `klend-sdk/src/classes/`

### Крок 2 — Scaffold (день 2)
- [ ] `programs/adapters/kamino/`
- [ ] Слідувати **Adapter Convention** (не Rust trait)

### Крок 3 — Obligation та `initialize_position` (день 2–3)

Kamino використовує `Obligation` акаунт для відстеження позиції кредитора:
```rust
// seeds точні — верифікувати через Kamino source code
// орієнтовно: [b"obligation", lending_market.key, adapter_authority.key]

#[account]
pub struct KaminoAdapterPosition {
    pub obligation: Pubkey,   // Kamino Obligation PDA
    pub bump: u8,
}
// seeds: [b"kamino_position", user.key]
```

- [ ] `initialize_position(user)` — окрема інструкція що:
  1. Створює `KaminoAdapterPosition` PDA (adapter-side)
  2. CPI `init_obligation` до Kamino (один раз)
  - **Не робити `init_if_needed` в deposit** — дозволяє re-init attack
- [ ] Зберегти `obligation` pubkey у `KaminoAdapterPosition`

> **`init_if_needed` небезпека:** якщо obligation ініціалізується автоматично в deposit, зловмисник може передати інший obligation акаунт і re-init чужу позицію. Явна `initialize_position` безпечніша.

### Крок 4 — Deposit інструкція (день 3–5)

Акаунти (приблизний список — верифікувати через IDL):
```rust
pub struct Deposit<'info> {
    pub owner: Signer<'info>,
    pub fee_payer: Signer<'info>,  // може збігатись з owner
    /// CHECK: Obligation PDA адаптера, verified by Kamino
    #[account(mut)]
    pub obligation: AccountInfo<'info>,
    /// CHECK: Lending market, verified by Kamino
    pub lending_market: AccountInfo<'info>,
    /// CHECK: Lending market authority PDA, verified by Kamino
    pub lending_market_authority: AccountInfo<'info>,
    /// CHECK: USDC reserve, verified by Kamino
    #[account(mut)]
    pub reserve: AccountInfo<'info>,
    /// CHECK: Reserve liquidity mint (USDC)
    pub reserve_liquidity_mint: AccountInfo<'info>,
    /// CHECK: Reserve liquidity supply vault
    #[account(mut)]
    pub reserve_liquidity_supply: AccountInfo<'info>,
    /// CHECK: Reserve collateral mint (kUSDC)
    #[account(mut)]
    pub reserve_collateral_mint: AccountInfo<'info>,
    /// CHECK: Destination for collateral tokens
    #[account(mut)]
    pub reserve_destination_deposit_collateral: AccountInfo<'info>,
    /// CHECK: Obligation collateral destination
    #[account(mut)]
    pub obligation_collateral_destination: AccountInfo<'info>,
    #[account(mut)]
    pub user_source_liquidity: AccountInfo<'info>, // user USDC ATA
    #[account(mut)]
    pub user_position: Account<'info, UserPosition>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    /// CHECK: Kamino Lending program ID
    pub kamino_program: AccountInfo<'info>,
}
```

> **Два Signer:** Якщо `owner` і `fee_payer` — один і той самий юзер, достатньо передати один акаунт двічі. Але якщо Kamino вимагає їх різними — потрібно два окремих підписи в транзакції.

- [ ] CPI `refresh_reserve` → `deposit` — в одній інструкції, атомарно
- [ ] Отримати кількість kTokens після CPI → `user_position.shares`
- [ ] ComputeBudget (400k units) — клієнтська інструкція

### Крок 5 — Withdraw інструкція (день 5–7)
- [ ] CPI `withdraw_obligation_collateral_and_redeem_reserve_collateral`
- [ ] Burn kTokens → отримати USDC
- [ ] Обробити часткове withdraw через `shares` proportion

### Крок 6 — current_value (день 7)

```rust
// kToken → USDC через collateral exchange rate
pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
    let reserve_data = ctx.accounts.reserve.try_borrow_data()?;
    let reserve = Reserve::try_deserialize(&mut &reserve_data[..])?;
    let position = &ctx.accounts.user_position;

    let exchange_rate = reserve.collateral_exchange_rate()?;
    // collateral_to_liquidity вже робить правильну u128 арифметику всередині
    let value = exchange_rate.collateral_to_liquidity(position.shares)?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
// Примітка: Reserve може бути stale якщо refresh_reserve не викликали нещодавно.
// current_value як view (simulateTransaction) це прийнятно.
```

- [ ] Десеріалізувати `Reserve` акаунт (великий struct ~500 байт)
- [ ] Використати `collateral_exchange_rate()` метод

### Крок 7 — Unit тести (день 8–10)
- [ ] Завантажити Kamino програму + Reserve акаунт з mainnet для LiteSVM
- [ ] Тест: deposit → kTokens отримано
- [ ] Тест: current_value зростає з часом (simulateSlots)
- [ ] Тест: withdraw → USDC повернуто

---

## Складнощі специфічні для Kamino

### Refresh Reserve
Перед будь-якою операцією Kamino вимагає `refresh_reserve` виклик **в тій самій транзакції**:
```rust
// Обов'язково перед deposit/withdraw — атомарно в одній tx
kamino::cpi::refresh_reserve(CpiContext::new(...), )?;
// потім одразу:
kamino::cpi::deposit_reserve_liquidity_and_obligation_collateral(...)?;
```
Це оновлює accumulated interest. `refresh_reserve` і deposit/withdraw — **два окремих CPI в одній інструкції**, не два окремих invoke.

### Refresh Obligation
Аналогічно, `refresh_obligation` перед withdraw в тій самій транзакції:
```rust
kamino::cpi::refresh_reserve(...)?;    // спочатку reserve
kamino::cpi::refresh_obligation(...)?; // потім obligation
kamino::cpi::withdraw_obligation_collateral_and_redeem_reserve_collateral(...)?;
```

> **CU бюджет:** 3 CPI в одній інструкції (refresh_reserve + refresh_obligation + withdraw) можуть вимагати 500k+ CU. Клієнт додає `ComputeBudgetProgram.setComputeUnitLimit({ units: 500_000 })` як першу інструкцію транзакції.

### Oracle залежність
Kamino використовує Scope oracle. Для тестів потрібно мати актуальні oracle акаунти або mock їх.

---

## Ризики

| Ризик | Вирішення |
|---|---|
| 15+ акаунтів у CPI | Використати `remaining_accounts` у Dispatcher |
| `refresh_reserve` вимога | Атомарно включити у кожну інструкцію |
| Камino оновлює IDL | Пін конкретного commit/version |
| Oracle stale у тестах | Seed mainnet oracle акаунти у Surfpool |

---

## Залежності

- Залежить від: Core Dispatcher (заморожений інтерфейс)
- Паралельно з: Jupiter LP, Maple

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| Дослідження IDL + акаунти | 2 дні |
| Obligation PDA + init | 1 день |
| Deposit | 2 дні |
| Withdraw | 2 дні |
| current_value | 1 день |
| Unit тести | 2 дні |
| **Разом** | **~10 днів** |

---

## Definition of Done

- [ ] `deposit` / `withdraw` / `current_value` реалізовані
- [ ] `refresh_reserve` та `refresh_obligation` включені
- [ ] Mainnet-fork тест проходить з реальними Kamino акаунтами
- [ ] kToken → USDC конвертація коректна
