# План: Jupiter LP Adapter

## Складність: ★★★★☆

## Мета

Адаптер для Jupiter Perpetuals LP (JLP) — liquidity pool токен що генерує yield від trading fees Jupiter Perp.

---

## Як працює Jupiter LP (контекст)

```
User USDC
    │  addLiquidity
    ▼
JLP Pool (multi-asset: USDC, SOL, ETH, wBTC, USDT)
    │  видає
    ▼
JLP Tokens (ERC4626-подібний pool share)
    │  нараховує yield від:
    │  - trading fees (open/close positions)
    │  - borrow fees від трейдерів
    │  - liquidation fees
    ▼
User USDC + yield (при removeLiquidity)
```

Ключові концепти:
- **JLP Pool** — один unified pool з 5 токенами: USDC, SOL, ETH, wBTC, USDT
- **JLP Token** — SPL токен `27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4`
- **AUM (Assets Under Management)** — сума всіх активів у pool у USD
- **JLP Price** = AUM / JLP total supply
- **Custodies** — окремі акаунти для кожного токена в pool

---

## Необхідні зовнішні залежності

```toml
# Cargo.toml — Jupiter Perp не має офіційного CPI crate
# Потрібно вручну описати account structs та discriminators
# або використати jupiter-perp IDL
```

```json
// package.json
"@jup-ag/perpetuals-sdk": "latest"  // якщо є публічно
// або "@jup-ag/api" для REST API підходу
```

**IDL:** Отримати через: `anchor idl fetch PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu`

---

## Mainnet акаунти

| Акаунт | Адреса |
|---|---|
| Jupiter Perp Program | `PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu` |
| JLP Pool | `5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq` |
| JLP Token Mint | `27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4` |
| USDC Custody | потрібно знайти через explorer |
| USDC Mint | `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` |

---

## Покроковий план реалізації

### Крок 1 — Дослідити Jupiter Perp програму (день 1–2)
- [ ] `anchor idl fetch PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu -o jupiter-perp.json`
- [ ] Знайти `addLiquidity` інструкцію та її акаунти
- [ ] Знайти `removeLiquidity` інструкцію та її акаунти
- [ ] Вивчити `Pool` та `Custody` structs для `current_value`
- [ ] Перевірити чи є oracle-залежності (Pyth price feeds)

### Крок 2 — Визначити підхід до pricing (день 2)

Jupiter Perp використовує власну oracle систему. Два варіанти:
```
Варіант A: Читати Pool.aum / JLP total_supply
           → потребує oracle-updated Pool акаунт
           
Варіант B: JLP Token ATA balance * JLP price (з oracle)
           → потребує окремий price feed
```
**Рекомендація:** Варіант A — читати напряму з on-chain Pool state.

### Крок 3 — Scaffold (день 2)
- [ ] `programs/adapters/jupiter-lp/`
- [ ] Визначити мінімальні structs для десеріалізації Pool та Custody
- [ ] Створити `adapter_jlp_ata` — **adapter's JLP ATA** (PDA-owned, holds JLP for all users):
  - Owner: adapter authority PDA (`[b"jupiter_authority"]`)
  - Mint: JLP Token Mint `27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4`
  - Ініціалізується **один раз** при деплої, не per-user
  - `UserPosition.shares` = частка JLP цього user у загальному adapter ATA
- [ ] Додати `initialize_adapter` інструкцію що створює `adapter_jlp_ata` (викликається при деплої)

### Крок 4 — Deposit (addLiquidity) інструкція (день 3–5)

Приблизні акаунти:
```rust
pub struct Deposit<'info> {
    pub owner: Signer<'info>,
    /// CHECK: user USDC ATA (source), verified by token program
    #[account(mut)]
    pub funding_account: AccountInfo<'info>,
    /// CHECK: ADAPTER's JLP ATA (destination — адаптер зберігає JLP від імені user)
    #[account(mut)]
    pub lp_token_account: AccountInfo<'info>,
    /// CHECK: Jupiter transfer authority PDA
    pub transfer_authority: AccountInfo<'info>,
    /// CHECK: Jupiter perpetuals state
    pub perpetuals: AccountInfo<'info>,
    /// CHECK: JLP Pool
    #[account(mut)]
    pub pool: AccountInfo<'info>,
    /// CHECK: USDC custody
    #[account(mut)]
    pub custody: AccountInfo<'info>,
    /// CHECK: USDC oracle
    pub custody_oracle_account: AccountInfo<'info>,
    /// CHECK: USDC custody token vault
    #[account(mut)]
    pub custody_token_account: AccountInfo<'info>,
    /// CHECK: JLP token mint
    #[account(mut)]
    pub lp_token_mint: AccountInfo<'info>,
    #[account(mut)]
    pub user_position: Account<'info, UserPosition>,
    pub token_program: Program<'info, Token>,
    /// CHECK: Jupiter Perp program ID
    pub jupiter_program: AccountInfo<'info>,
    // remaining_accounts: [custody_0, oracle_0, custody_1, oracle_1, ...]
    // порядок відповідає pool.custodies array
}
```

> **Signer flow через CPI:** User підписує транзакцію → Dispatcher CPI до Adapter → Adapter CPI до Jupiter. Підпис user пропагується через `invoke_signed` (Dispatcher → Adapter через PDA), але **user підпис доступний на рівні Adapter** тільки якщо Dispatcher передає його через `remaining_accounts` як signer. Потрібно верифікувати через тестування.

> **Адаптер зберігає JLP:** JLP токени йдуть на **adapter's JLP ATA** (PDA адаптера), не на user's ATA. Адаптер відстежує скільки JLP належить кожному user через `UserPosition.shares`.

- [ ] CPI `addLiquidity` з усіма custody oracle акаунтами у `remaining_accounts`
- [ ] Прочитати баланс adapter JLP ATA до і після CPI → `user_position.shares += delta`
- [ ] ComputeBudget (500k units) — клієнтська інструкція

### Крок 5 — Withdraw (removeLiquidity) інструкція (день 5–7)
- [ ] CPI `removeLiquidity` — аналогічний набір акаунтів
- [ ] Вказати `outputMint = USDC` щоб отримати USDC
- [ ] Обробити `minAmount` parameter (slippage protection)
- [ ] Emit `Withdrawn` event

### Крок 6 — current_value (день 7)

```rust
// JLP value = user_jlp_balance * pool.aum_usd / JLP_total_supply
// ВАЖЛИВО: множення ПЕРЕД діленням щоб уникнути precision loss.
// Проміжний результат може бути великим → використовуємо u128.
pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
    let pool = Pool::try_deserialize(...)?;
    let jlp_mint = Mint::try_deserialize(...)?;
    let position = &ctx.accounts.user_position;

    // u128 для запобігання overflow при множенні
    let value_usd = (position.shares as u128)
        .checked_mul(pool.aum_usd as u128)
        .and_then(|v| v.checked_div(jlp_mint.supply as u128))
        .and_then(|v| u64::try_from(v).ok())   // безпечний cast, не as u64
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value_usd.to_le_bytes());
    Ok(())
}
```

> **Precision:** Порядок операцій критичний. `aum / supply * shares` втрачає точність через integer division першою. `shares * aum / supply` зберігає точність. При shares=1 і aum/supply=1.5 USDC — перший варіант дає 1, другий дає 1 (різниця незначна, але при великих обсягах помітна).

**Примітка:** `pool.aum_usd` оновлюється при кожній торговій операції в протоколі. Для стейлих значень прийнятно для нашого use-case.

### Крок 7 — Oracle акаунти для тестів (день 7–8)
- [ ] Seeded mainnet акаунти для всіх 5 oracle feeds у Surfpool
- [ ] SOL/USD, ETH/USD, BTC/USD, USDC/USD price feeds
- [ ] Перевірити що oracle акаунти не stale (є max age)

### Крок 8 — Unit тести (день 8–10)
- [ ] LiteSVM з Jupiter Perp програмою (бінарний завантаж з mainnet)
- [ ] Тест: deposit USDC → JLP tokens отримано
- [ ] Тест: current_value = коректне USD значення
- [ ] Тест: withdraw JLP → USDC повернуто

---

## Специфічні виклики

### Multi-asset oracle requirement
```rust
// Jupiter потребує oracle для КОЖНОГО активу в pool при розрахунку AUM
// Всі 5 oracle акаунти мають бути передані у remaining_accounts
// Порядок важливий — відповідає custody порядку в Pool
```

### JLP → USDC conversion при withdraw
Jupiter дозволяє обрати output token. Ми завжди виводимо в USDC:
```rust
// removeLiquidity params
// output_mint = USDC_MINT
// min_amount_out = amount * (1 - slippage)  // 0.5% slippage
```

### Decimals
- USDC: 6 decimals
- JLP Token: 6 decimals
- AUM у Pool: потрібно перевірити decimals

---

## Ризики

| Ризик | Вирішення |
|---|---|
| Неповний публічний IDL | Реверс-інжиніринг з on-chain bytecode |
| Oracle stale у fork тестах | Seed свіжі oracle значення перед тестом |
| Slippage при великому withdraw | Додати `min_amount_out` параметр |
| AUM застаріває між операціями | Документувати як best-effort approximation |

---

## Залежності

- Залежить від: Core Dispatcher (заморожений)
- Паралельно з: Kamino, Maple

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| Дослідження IDL + oracle structure | 2 дні |
| Deposit (addLiquidity) | 3 дні |
| Withdraw (removeLiquidity) | 2 дні |
| current_value | 1 день |
| Oracle seeding для тестів | 1 день |
| Unit тести | 2 дні |
| **Разом** | **~11 днів** |

---

## Definition of Done

- [ ] `deposit` / `withdraw` / `current_value` реалізовані
- [ ] Всі oracle акаунти правильно передані в CPI
- [ ] Mainnet-fork тест проходить
- [ ] JLP → USDC conversion коректна при withdraw
- [ ] Slippage задокументований у spec
