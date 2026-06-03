# План: On-Chain Adapter Registry

## Мета

On-chain реєстр адаптерів з governance-gated механізмом додавання/видалення. Гарантує що Dispatcher взаємодіє лише з верифікованими адаптерами.

---

## Архітектура

```
Governance Authority (multisig / DAO)
        │
        ▼
   Registry Program
        │  approve_adapter / revoke_adapter
        ▼
  RegistryEntry PDA
  [adapter_program_id → metadata]
        │
        ▼
  Dispatcher Program
  (читає Registry перед CPI)
```

---

## Акаунти

```rust
#[account]
pub struct RegistryState {
    pub authority: Pubkey,         // governance authority (multisig)
    pub pending_authority: Pubkey, // для 2-step authority transfer
    pub bump: u8,
    // adapter_count видалено: лічильник не може бути надійно синхронізований
    // з is_active/is_paused станом. Клієнт рахує активні entries через getProgramAccounts.
}
// space = 8 + 32 + 32 + 1 = 73 bytes

#[account]
pub struct RegistryEntry {
    pub adapter_program_id: Pubkey,
    pub name: [u8; 64],            // назва адаптера (64 байти — достатньо для довгих назв)
    pub protocol: [u8; 64],        // назва протоколу
    pub asset_mint: Pubkey,        // токен що приймає адаптер (USDC, etc)
    pub version: u16,              // версія адаптера (інкрементується при upgrade)
    pub is_active: bool,           // false = revoked (постійно)
    pub is_paused: bool,           // true = тимчасово призупинений
    pub approved_at: i64,          // unix timestamp
    pub approved_by: Pubkey,
    pub bump: u8,
}
// space = 8 + 32 + 64 + 64 + 32 + 2 + 1 + 1 + 8 + 32 + 1 = 245 bytes
```

> **`revoke` vs `pause`:** `revoke` — постійне видалення (is_active = false). `pause` — тимчасове вимкнення при exploit або обслуговуванні (is_paused = true, is_active залишається true). Dispatcher перевіряє обидва: `is_active && !is_paused`.

PDA для `RegistryEntry`: `[b"registry_entry", adapter_program_id]`

---

## Покроковий план реалізації

### Крок 1 — Ініціалізація Registry (день 1)
- [ ] `initialize_registry(authority: Pubkey)` — створює `RegistryState`
- [ ] Authority = Pubkey що деплоює (для devnet — deployer keypair)
- [ ] Тест: RegistryState створено з правильним authority

### Крок 2 — Approve механізм (день 2–3)
- [ ] `approve_adapter(adapter_program_id, name, protocol, asset_mint)`
  - Signer: `authority` з RegistryState
  - Створює `RegistryEntry` PDA
  - Встановлює `is_active = true`
- [ ] Тест: approve → RegistryEntry існує і is_active
- [ ] Тест: не-authority підписант → `Unauthorized` error

### Крок 3 — Revoke та Pause механізми (день 3)
- [ ] `revoke_adapter(adapter_program_id)`
  - Signer: `authority`
  - Встановлює `is_active = false` (постійно)
  - **Не закриває PDA** — запис залишається як аудит-лог
  - Якщо потрібно повторно approve той самий program_id — використовувати `close_registry_entry` спочатку
- [ ] `close_registry_entry(adapter_program_id)`
  - Signer: `authority`
  - Тільки для revoked (`is_active == false`) записів
  - Закриває PDA, повертає rent authority
  - Після цього можна approve той самий program_id знову
- [ ] `pause_adapter(adapter_program_id)`
  - Signer: `authority`
  - Встановлює `is_paused = true` (тимчасово)
- [ ] `resume_adapter(adapter_program_id)`
  - Signer: `authority`
  - Встановлює `is_paused = false`
- [ ] Тест: revoke → is_active = false → Dispatcher відхиляє
- [ ] Тест: pause → is_paused = true → Dispatcher відхиляє
- [ ] Тест: resume → Dispatcher знову приймає
- [ ] Тест: revoked адаптер не може бути resumed (is_active перевіряється)

### Крок 4 — Authority transfer (день 4)
- [ ] `propose_authority(new_authority: Pubkey)` — записує `pending_authority`
- [ ] `accept_authority()` — new_authority підтверджує, стає authority
- [ ] 2-step pattern для безпеки (уникає помилки з неправильним адресом)
- [ ] Тест: повний transfer flow

### Крок 5 — Update та View (день 4–5)
- [ ] `update_adapter(adapter_program_id, name: Option<[u8;64]>, protocol: Option<[u8;64]>, asset_mint: Option<Pubkey>)`
  - Signer: `authority`
  - Anchor: параметри як `Option<T>` — якщо `None`, поле не змінюється
  - Оновлює метадані без revoke+re-approve, автоматично інкрементує `version`
- [ ] Клієнтський TypeScript helper: `isAdapterActive(programId)` — перевіряє `is_active && !is_paused`
- [ ] Клієнтський TypeScript helper: `getAllActiveAdapters()` — getProgramAccounts фільтр
- [ ] Клієнтський TypeScript helper: `getAdapterVersion(programId)` → `version: u16`

### Крок 6 — Інтеграція з Dispatcher (день 5–6)
- [ ] Dispatcher читає `RegistryEntry` PDA у кожній інструкції
- [ ] Перевірка: `entry.is_active == true && entry.is_paused == false`
- [ ] Перевірка: `entry.adapter_program_id == передана програма`
- [ ] Cross-program account read (не CPI — просто читання акаунта через `AccountInfo`)
- [ ] Додати `registry_entry: Account<RegistryEntry>` до кожного `#[derive(Accounts)]` у Dispatcher з `owner` constraint:
  ```rust
  #[account(owner = REGISTRY_PROGRAM_ID @ AdapterError::AdapterNotRegistered)]
  pub registry_entry: Account<'info, RegistryEntry>,
  ```
  Без `owner` constraint — атакуючий може підсунути фейковий акаунт з `is_active = true`.

### Крок 7 — Devnet deploy (день 6–7)
- [ ] `NO_DNA=1 anchor deploy --provider.cluster devnet`
- [ ] Ініціалізація Registry на devnet
- [ ] Approve всіх 5 адаптерів після їх деплою
- [ ] Верифікація через `anchor idl fetch`

---

## Структура файлів

```
programs/
└── registry/
    ├── src/
    │   ├── lib.rs
    │   ├── instructions/
    │   │   ├── initialize.rs
    │   │   ├── approve_adapter.rs
    │   │   ├── revoke_adapter.rs
    │   │   ├── close_registry_entry.rs
    │   │   ├── pause_adapter.rs
    │   │   ├── resume_adapter.rs
    │   │   ├── update_adapter.rs
    │   │   ├── propose_authority.rs
    │   │   ├── accept_authority.rs
    │   │   └── mod.rs
    │   ├── state/
    │   │   ├── registry_state.rs
    │   │   ├── registry_entry.rs
    │   │   └── mod.rs
    │   └── error.rs
    └── Cargo.toml
```

---

## Governance: рівні складності

### Мінімальний (для submission) — Single Authority
Один Pubkey керує реєстром. Для devnet достатньо.

### Розширений (опціонально) — Multisig
Інтеграція з [Squads Protocol](https://squads.so/) як multisig authority. Не змінює логіку Registry — просто authority стає Squads vault адресою.

### Повний DAO (out of scope)
SPL Governance + voting — поза scope цього проекту.

**Рекомендація:** реалізувати Single Authority, але спроектувати так щоб authority міг бути Squads multisig без змін в програмі.

---

## Events для indexing

> **Тип конвертація у events:** `RegistryEntry` зберігає `protocol: [u8; 64]`, але events використовують `String` для зручності indexing. При emit потрібна явна конвертація:
> ```rust
> let protocol = String::from_utf8_lossy(&entry.protocol)
>     .trim_end_matches('\0').to_string();
> emit!(AdapterApproved { protocol, ... });
> ```

```rust
#[event]
pub struct AdapterApproved {
    pub adapter_program_id: Pubkey,
    pub protocol: String,
    pub approved_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdapterRevoked {
    pub adapter_program_id: Pubkey,
    pub revoked_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdapterPaused {
    pub adapter_program_id: Pubkey,
    pub paused_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdapterResumed {
    pub adapter_program_id: Pubkey,
    pub resumed_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdapterUpdated {
    pub adapter_program_id: Pubkey,
    pub new_version: u16,
    pub updated_by: Pubkey,
    pub timestamp: i64,
}
```

---

## Залежності

- Залежить від: фіналізованого інтерфейсу Core Dispatcher
- Не залежить від: конкретних адаптерів
- Dispatcher залежить від: Registry (читає RegistryEntry)

---

## Оцінка часу

| Завдання | Часу |
|---|---|
| Initialize + Approve/Revoke | 2 дні |
| Pause/Resume + Update | 1 день |
| Authority transfer | 1 день |
| Dispatcher інтеграція | 1 день |
| Devnet deploy + verify | 2 дні |
| **Разом** | **~7 днів** |

---

## Definition of Done

- [ ] Registry задеплоєний на devnet
- [ ] Всі 5 адаптерів зареєстровані в Registry на devnet
- [ ] `approve_adapter` / `revoke_adapter` / `pause_adapter` / `resume_adapter` мають unit тести
- [ ] `update_adapter` інкрементує version
- [ ] Authority transfer протестований
- [ ] Dispatcher перевіряє `is_active && !is_paused`
- [ ] TypeScript helpers: `isAdapterActive`, `getAllActiveAdapters`, `getAdapterVersion`
- [ ] `register-adapter.ts` скрипт для CLI реєстрації
