# План: MarginFi USDC Adapter

## Складність: ★★★☆☆ (Стартовий адаптер)

## Мета

Перший адаптер для відпрацювання загального патерну. MarginFi — lending protocol з добре задокументованим SDK та відкритим IDL.

---

## Як працює MarginFi (контекст)

```
User USDC
    │  deposit (CPI через адаптер)
    ▼
MarginFi Bank Vault (USDC lending pool)
    │  видає внутрішні
    ▼
Bank Shares (interest-bearing, зберігаються у MarginfiAccount адаптера)
    │  нараховує yield (exchange rate зростає з часом)
    ▼
User USDC + interest  (при withdraw)
```

> **Примітка:** MarginFi не видає окремий SPL-токен для USDC-депозитів. Shares — це внутрішній запис у `MarginfiAccount`. Не плутати з mSOL (це токен Marinade Finance для SOL staking).

Ключові концепти:
- **MarginfiGroup** — верхній рівень, містить конфіг
- **MarginfiAccount** — позиція конкретного юзера
- **Bank** — пул ліквідності для конкретного токена (USDC Bank)
- **Shares** — внутрішня одиниця обліку, конвертується в assets за exchange rate

---

## Необхідні зовнішні залежності

```toml
# Cargo.toml
marginfi = { git = "https://github.com/mrgnlabs/marginfi-v2", features = ["no-entrypoint"] }
```

```json
// package.json
"@mrgnlabs/marginfi-client-v2": "latest"
```

**IDL:** Отримати з mainnet: `anchor idl fetch MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA`

---

## Mainnet акаунти (USDC)

| Акаунт | Адреса |
|---|---|
| MarginFi Program | `MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA` |
| MarginFi Group | `4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG` |
| USDC Bank | `2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB` |
| USDC Mint | `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` |

---

## Покроковий план реалізації

### Крок 1 — Вивчити MarginFi CPI (день 1)
- [ ] Прочитати [marginfi-v2 програму](https://github.com/mrgnlabs/marginfi-v2)
- [ ] Знайти `lending_account_deposit` та `lending_account_withdraw` інструкції
- [ ] Визначити всі необхідні акаунти для кожної інструкції
- [ ] Перевірити discriminator для CPI

### Крок 2 — Scaffold адаптера (день 1–2)
- [ ] `anchor init marginfi-adapter` у `programs/adapters/marginfi/`
- [ ] Додати залежність від `adapter-interface` shared crate
- [ ] Слідувати **Adapter Convention** (не Rust trait — Anchor інструкції не impl-ять trait)
- [ ] Додати `AdapterPosition` PDA для MarginFi-specific state:
  ```rust
  #[account]
  pub struct MarginfiAdapterPosition {
      pub marginfi_account: Pubkey,  // PDA у MarginFi
      pub bump: u8,
  }
  // space = 8 + 32 + 1 = 41 bytes
  // seeds: [b"marginfi_position", user.key, adapter.key]
  ```

### Крок 3 — Deposit інструкція (день 2–3)

Необхідні акаунти:
```rust
#[derive(Accounts)]
pub struct Deposit<'info> {
    /// CHECK: verified as writable by MarginFi program
    #[account(mut)]
    pub marginfi_group: AccountInfo<'info>,
    /// CHECK: PDA адаптера у MarginFi, verified by seeds
    #[account(mut)]
    pub marginfi_account: AccountInfo<'info>,
    #[account(mut)]
    pub signer: Signer<'info>,
    /// CHECK: USDC bank, verified as writable by MarginFi program
    #[account(mut)]
    pub bank: AccountInfo<'info>,
    /// CHECK: user USDC ATA (source), verified by token program during transfer
    #[account(mut)]
    pub signer_token_account: AccountInfo<'info>,
    /// CHECK: MarginFi vault (destination), verified by MarginFi program
    #[account(mut)]
    pub bank_liquidity_vault: AccountInfo<'info>,
    #[account(mut)]
    pub user_position: Account<'info, UserPosition>, // Dispatcher акаунт
    pub token_program: Program<'info, Token>,
    /// CHECK: verified as MarginFi program ID
    pub marginfi_program: AccountInfo<'info>,
}
```

> **Funds flow:** User USDC ATA → MarginFi Bank Vault (напряму через CPI). Адаптер підписує як `marginfi_account` owner через PDA signer — ніякого проміжного "Adapter Token Account" не потрібно.
>
> **`/// CHECK:`** — обов'язкова анотація для кожного `AccountInfo` без типізованого constraint. Без неї `anchor build` видає помилку.

- [ ] CPI `lending_account_deposit` з `CpiContext::new_with_signer(..., &[&[seeds, &[bump]]])`
- [ ] Прочитати shares з `MarginfiAccount.lending_account.balances` після CPI → `user_position.shares`
- [ ] Emit `Deposited` event
- [ ] ComputeBudget встановлюється клієнтом (350k units) — не через CPI

### Крок 4 — Withdraw інструкція (день 3–4)

- [ ] CPI виклик `lending_account_withdraw`
- [ ] Обробити часткове та повне withdraw
- [ ] Оновити `UserPosition`
- [ ] Emit `Withdrawn` event

### Крок 5 — current_value (день 4)

```rust
// shares * asset_share_value = current_usdc_value
// asset_share_value зберігається в Bank акаунті (оновлюється з кожним блоком)
pub fn current_value(ctx: Context<CurrentValue>) -> Result<()> {
    // try_borrow_data() безпечніший за data.borrow() — повертає Result, не panic
    let data = ctx.accounts.bank.try_borrow_data()?;
    let bank = Bank::try_deserialize(&mut &data[..])?;
    let position = &ctx.accounts.user_position;

    // user_shares * total_assets / total_shares — порядок важливий (mul before div)
    let value = (position.shares as u128)
        .checked_mul(bank.total_asset_value()? as u128)
        .and_then(|v| v.checked_div(bank.total_shares() as u128))
        .and_then(|v| u64::try_from(v).ok())
        .ok_or(error!(AdapterError::Overflow))?;

    set_return_data(&value.to_le_bytes());
    Ok(())
}
```

- [ ] `try_borrow_data()` замість `.data.borrow()` — безпечніше при паралельному доступі
- [ ] `u128` арифметика з `mul-before-div` для точності
- [ ] `u64::try_from` замість `as u64` — поверне error замість panic при overflow

### Крок 6 — Ініціалізація (день 4–5)

> **Два окремих виклики перед першим deposit (різні програми):**
> 1. `dispatcher::initialize_position(adapter: Pubkey)` — створює `UserPosition` PDA у **Dispatcher** (seeds: `[b"position", user.key, adapter.key]`)
> 2. `marginfi_adapter::initialize_position()` — створює `MarginfiAdapterPosition` PDA у **MarginFi adapter** та CPI `init_marginfi_account` до MarginFi
>
> Жодна програма не може ініціалізувати PDA іншої — це два окремих `initialize_position` від двох різних програм.

- [ ] `dispatcher::initialize_position(adapter)` — seeds `[b"position", user.key, adapter.key]`
- [ ] `marginfi_adapter::initialize_position()` — seeds `[b"marginfi_position", user.key, adapter.key]`, CPI `init_marginfi_account`
- [ ] MarginfiAccount PDA seeds: верифікувати реальні seeds через MarginFi source code (не припускати `[b"marginfi_account", adapter_program_id]`)
- [ ] Перевірити `if marginfi_account.data_is_empty() { initialize }` перед CPI deposit

### Крок 7 — Unit тести з LiteSVM (день 5–7)
- [ ] Завантажити MarginFi програму з mainnet для LiteSVM
- [ ] Mock USDC mint та bank акаунти
- [ ] Тест: deposit → перевірити shares > 0
- [ ] Тест: deposit → withdraw → перевірити USDC повернуто
- [ ] Тест: current_value після deposit > deposited amount (після часу)

---

## Структура файлів

```
programs/adapters/marginfi/
├── src/
│   ├── lib.rs
│   ├── instructions/
│   │   ├── deposit.rs
│   │   ├── withdraw.rs
│   │   ├── current_value.rs
│   │   └── mod.rs
│   ├── cpi/
│   │   └── marginfi.rs       — CPI helpers
│   ├── state.rs
│   └── error.rs
└── Cargo.toml
```

---

## Funds Flow

```
User USDC ATA
 │ CPI lending_account_deposit
 │ (адаптер підписує як marginfi_account owner)
 ▼
MarginFi Bank Vault (USDC)
 │ оновлює shares у
 ▼
MarginfiAccount (PDA адаптера)
 │ адаптер читає нові shares і записує до
 ▼
UserPosition.shares (Dispatcher акаунт)
```

---

## Ризики та вирішення

| Ризик | Вирішення |
|---|---|
| MarginFi оновлює інтерфейс | Пін версії IDL та git commit hash |
| `MarginfiAccount` вже існує | Перевіряти перед `initialize` |
| Rounding у share calculation | Використовувати `checked_math` |

---

## Залежності

- Залежить від: Core Dispatcher interface (frozen)
- Не залежить від: інших адаптерів
- Registry: approval після деплою

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| Вивчення MarginFi CPI | 1 день |
| Deposit + Withdraw | 3 дні |
| current_value | 1 день |
| Unit тести | 2 дні |
| **Разом** | **~7 днів** |

---

## Definition of Done

- [ ] `deposit` / `withdraw` / `current_value` реалізовані
- [ ] LiteSVM unit тести проходять
- [ ] Mainnet-fork тест проходить (Surfpool)
- [ ] Адаптер зареєстрований у Registry на devnet
