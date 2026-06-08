# План: Drift Insurance Fund Adapter

> ⚠️ **ІСТОРИЧНИЙ ПЛАН — ЧАСТКОВО ЗАСТАРІВ.** Цей документ описує первісний задум (IF-стейкінг),
> написаний до виявлення блокера. Фактично: (1) IF-стейкінг на мейннеті вимкнено, тож адаптер
> переписано на **USDC spot-market lending** (single-step, без cooldown); (2) це не допомогло —
> Drift закоментував **усі** інструкції проґраму (`comment out all ixs`), тож будь-який CPI дає
> `101` і справжній fork-тест неможливий. Актуальний стан і докази (вкл. сорс-пруф) —
> [`../../troubleshooting/drift-fork-issues.md`](../../troubleshooting/drift-fork-issues.md).
> Розділи нижче про IF-механіку/cooldown лишаються як історія задуму.

## Складність: ★★★☆☆

## Мета

Адаптер для стейкінгу в Drift Protocol Insurance Fund. Другий адаптер — підтверджує що патерн з MarginFi масштабується.

---

## Як працює Drift Insurance Fund (контекст)

```
User USDC
    │  stake
    ▼
Insurance Fund Vault (per-market або spot market)
    │  нараховує yield від trading fees + liquidations
    ▼
IF Shares (insurance fund shares)
    │  unstake (з cooldown)
    ▼
User USDC + yield
```

Ключові концепти:
- **Insurance Fund** — буфер проти bad debt у Drift
- **IF Shares** — одиниці стейкінгу, ціна зростає коли vault отримує доходи
- **Unstake cooldown** — є затримка при виході (важливо для `withdraw`)
- **Spot Market** — стейкінг прив'язаний до конкретного spot market (USDC = market 0)

---

## Необхідні зовнішні залежності

```toml
# Cargo.toml
drift = { git = "https://github.com/drift-labs/protocol-v2", features = ["no-entrypoint", "mainnet-beta"] }
```

```json
// package.json
"@drift-labs/sdk": "latest"
```

**IDL:** `anchor idl fetch dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH`

---

## Mainnet акаунти

| Акаунт | Адреса |
|---|---|
| Drift Program | `dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH` |
| State | `FxztXFc4aiyHzGHCTzJRys5X4TnWVBzJTYeSMn7b65nA` |
| USDC Spot Market (index 0) | потрібно отримати через SDK |
| IF Vault USDC | потрібно отримати через SDK |
| USDC Mint | `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` |

---

## Покроковий план реалізації

### Крок 1 — Вивчити IF механізм (день 1)
- [ ] Прочитати `drift/programs/drift/src/instructions/insurance_fund_stake.rs`
- [ ] Зрозуміти: `add_insurance_fund_stake`, `request_remove_insurance_fund_stake`, `remove_insurance_fund_stake`
- [ ] Зрозуміти cooldown механізм (є `unstake_request` → очікування → `remove`)
- [ ] Знайти всі необхідні акаунти

### Крок 2 — Scaffold (день 1)
- [ ] `programs/adapters/drift/`
- [ ] Слідувати **Adapter Convention** (не Rust trait)
- [ ] Додати `DriftAdapterPosition` PDA для Drift-specific state:
  ```rust
  #[account]
  pub struct DriftAdapterPosition {
      pub if_shares: u128,               // u128 — відповідає Drift IF shares type
      pub market_index: u16,             // USDC = 0 (зберігається для гнучкості)
      pub pending_withdrawal: bool,
      pub withdrawal_request_shares: u128,
      pub withdrawal_request_ts: i64,
      pub bump: u8,
  }
  // space = 8 + 16 + 2 + 1 + 16 + 8 + 1 = 52 bytes
  // seeds: [b"drift_position", user.key]
  ```
  > **Примітка:** `UserPosition.shares` (Dispatcher) зберігає `u64` для сумісності.
  > `DriftAdapterPosition.if_shares` зберігає повний `u128` для точного обліку.

### Крок 3 — InsuranceFundStake PDA (день 2)
- [ ] Drift створює `InsuranceFundStake` акаунт для кожного стейкера
- [ ] PDA: визначити через Drift SDK
- [ ] `initialize_insurance_fund_stake` — якщо не існує

### Крок 4 — Deposit (stake) інструкція (день 2–3)

Акаунти для `add_insurance_fund_stake`:
```rust
pub struct Deposit<'info> {
    /// CHECK: Drift state, verified by Drift program
    pub state: AccountInfo<'info>,
    /// CHECK: USDC spot market (index 0), verified by Drift program
    #[account(mut)]
    pub spot_market: AccountInfo<'info>,
    /// CHECK: IF vault for USDC, verified by Drift program
    #[account(mut)]
    pub insurance_fund_vault: AccountInfo<'info>,
    /// CHECK: InsuranceFundStake PDA for this adapter, verified by Drift
    #[account(mut)]
    pub insurance_fund_stake: AccountInfo<'info>,
    /// CHECK: user USDC ATA, verified by token program during transfer
    #[account(mut)]
    pub user_token_account: AccountInfo<'info>,
    #[account(mut)]
    pub user_position: Account<'info, UserPosition>,
    #[account(mut)]
    pub drift_adapter_position: Account<'info, DriftAdapterPosition>,
    pub token_program: Program<'info, Token>,
    /// CHECK: Drift program ID
    pub drift_program: AccountInfo<'info>,
}
```

- [ ] Зберегти `market_index: u16 = 0` у `DriftAdapterPosition` (не хардкодити в бізнес-логіці — краще константа)
- [ ] CPI `add_insurance_fund_stake(amount, market_index)`
- [ ] Прочитати `InsuranceFundStake.if_shares` після CPI → `drift_adapter_position.if_shares` (u128)
- [ ] Записати truncated u64 → `user_position.shares` (для сумісності з Dispatcher інтерфейсом)

### Крок 5 — Withdraw (unstake) з cooldown (день 3–5)

**Важливо:** Drift має 2-step unstake:
1. `request_remove_insurance_fund_stake` — ініціює cooldown (N епох)
2. `remove_insurance_fund_stake` — виконує після cooldown

Стан-машина у `withdraw`:
```rust
pub fn withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()> {
    let pos = &mut ctx.accounts.drift_adapter_position; // mut — змінюємо стан
    let now = Clock::get()?.unix_timestamp;             // один виклик до if/else
    let cooldown = read_cooldown_from_spot_market(&ctx)?; // читає SpotMarket.insurance_fund.unstaking_period

    if pos.pending_withdrawal {
        // Крок 2: cooldown пройшов?
        require!(
            now >= pos.withdrawal_request_ts + cooldown,
            AdapterError::CooldownActive
        );
        // Виконати remove
        drift::cpi::remove_insurance_fund_stake(...)?;
        pos.pending_withdrawal = false;
        emit!(UnstakeCompleted { ... });
    } else {
        // Крок 1: ініціювати request
        drift::cpi::request_remove_insurance_fund_stake(...)?;
        pos.pending_withdrawal = true;
        pos.withdrawal_request_ts = now;               // використовуємо вже отримане значення
        pos.withdrawal_request_shares = pos.if_shares;
        emit!(UnstakeRequested { ready_at: now + cooldown }); // cooldown доступний тут
    }
    Ok(())
}
```

- [ ] `DriftAdapterPosition.pending_withdrawal` — стан cooldown (не `UserPosition`)
- [ ] Emit `UnstakeRequested` event з `ready_at` timestamp
- [ ] Emit `UnstakeCompleted` event при фінальному withdraw

### Крок 6 — current_value (день 5)

```rust
// IF shares * vault_balance / total_shares = current_usdc_value
// Всі операції в u128, if_vault передається як акаунт у CurrentValue context
pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
    let spot_market_data = ctx.accounts.spot_market.try_borrow_data()?;
    let spot_market = SpotMarket::try_deserialize(&mut &spot_market_data[..])?;
    let pos = &ctx.accounts.drift_adapter_position;

    let vault_balance = ctx.accounts.if_vault.amount as u128; // if_vault у accounts
    let total_shares = spot_market.insurance_fund.total_shares; // u128

    require!(total_shares > 0, AdapterError::ProtocolError);

    let value = pos.if_shares                     // u128 — не u64
        .checked_mul(vault_balance)
        .and_then(|v| v.checked_div(total_shares))
        .and_then(|v| u64::try_from(v).ok())      // безпечний cast
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
// Акаунти CurrentValue повинні включати: spot_market, if_vault, drift_adapter_position
```

### Крок 7 — Unit тести (день 5–7)
- [ ] Тест: deposit → shares > 0 (перевірити що `if_shares` типу u128 правильно зберігається)
- [ ] Тест: request withdraw → `UnstakeRequested` emitted, `user_position.pending_withdrawal = true`
- [ ] Тест: withdraw before cooldown → `CooldownActive` error
- [ ] Тест: current_value = правильний розрахунок (u128 арифметика)
- [ ] Тест: overflow scenario — великий vault balance не переповнює u128

---

## Cooldown handling у клієнтському коді

```typescript
// TypeScript клієнт має перевіряти стан перед withdraw
const stake = await driftClient.getInsuranceFundStake(marketIndex);
if (stake.lastWithdrawRequestShares.gt(ZERO)) {
  // cooldown в процесі
  const readyAt = stake.lastWithdrawRequestTs + cooldownPeriod;
  console.log(`Ready at: ${new Date(readyAt * 1000)}`);
}
```

---

## Особливості та ризики

| Особливість | Вирішення |
|---|---|
| Cooldown при unstake | Документувати у spec, 2-step withdraw |
| Cooldown duration змінний | Читати з `SpotMarket.insuranceFund.unstakingPeriod` (зараз ~13 днів) |
| Slash ризик | Документувати — IF може бути slashed при bad debt |
| market_index варіюється | Хардкодити USDC = market index 0 |

---

## Залежності

- Залежить від: Core Dispatcher interface
- Не залежить від: MarginFi або інших адаптерів

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| Вивчення IF механізму | 1 день |
| Deposit | 2 дні |
| Withdraw (2-step) | 2 дні |
| current_value | 1 день |
| Unit тести | 2 дні |
| **Разом** | **~8 днів** |

---

## Definition of Done

- [ ] Deposit (stake) працює проти mainnet-fork
- [ ] Withdraw (request + remove) коректно обробляє cooldown
- [ ] current_value повертає правильне значення
- [ ] Cooldown поведінка задокументована у spec
- [ ] Mainnet-fork тест проходить
