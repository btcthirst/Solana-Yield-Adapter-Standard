# Dispatcher (e2e) та Maple — проблеми при mainnet-fork тестуванні

## Dispatcher — `tests/fork/06_dispatcher.ts`

> Статус: **ВИРІШЕНО** — зелений 8/8 на свіжому surfpool. Commit: `4c7e5b1` (+ SPEC/error-коди в попередньому).

### 1. ⭐ `OperationWithdrawOnly` (6020) від MarginFi на withdraw через Dispatcher

**Симптом:** standalone MarginFi withdraw(0) проходить, але **через Dispatcher** — падає з marginfi `OperationWithdrawOnly`.

**Корінь:** Dispatcher розгортав `withdraw(0)` → `withdraw(pos_shares)` ДО CPI в адаптер. Тож адаптер отримував non-zero → ішов у **partial-withdraw** (`withdraw_all=false`), лишаючи залишок, який marginfi відхиляв. А прямий `withdraw(0)` → адаптер бачить 0 → `withdraw_all=true` → закриває позицію → ОК.

**Фікс:** передавати `shares` **як є** (0 = withdraw-all обробляє адаптер), як і вказано в SPEC §2.2. Перевірку на over-withdraw для non-zero лишити.

### 2. Хибний код помилки в `close_position`

`close_position` використовував `AdapterNotRegistered` для constraint невідповідності owner.
**Фікс:** додано `DispatcherError::Unauthorized = 205`.

### 3. Неточності SPEC по кодах помилок

- `AdapterNotRegistered (6200)` недосяжний (відсутній entry → Anchor `AccountNotInitialized 3012`) → позначено reserved.
- Registry `AlreadyRegistered`/`NotFound` — reserved (їх кидає вбудований Anchor init/seeds).
- Додано рядок 6205 `Unauthorized`.

### 4. Не-ідемпотентність на персистентному surfpool

06 бутстрапить singleton-Registry і **revoke'ає адаптер у кінці**, плюс новий owner щоразу. Тож повторний запуск на тому ж surfpool колізить (registry «already in use», AdapterRevoked). **Це by design** — набір розрахований на **свіжий форк на кожен запуск** (саме так робить CI; L3-зміни піднімають surfpool наново). Не баг.

---

## Maple syrupUSDC — `tests/fork/05_maple.ts`

> Статус: **ЗЕЛЕНИЙ 4/4 без fork-блокерів.** Єдиний адаптер, що пройшов одразу.

**Чому без проблем:** Maple-адаптер **не робить зовнішнього протокольного CPI** — він кустодіює syrupUSDC в authority-ATA (deposit = SPL-transfer від юзера, withdraw = SPL-transfer назад, обидва — нативний token program). Немає чужих дискримінаторів, offset'ів, оракулів чи farms.

**Єдина правка (якість, не fork-блокер) — M3, попередній commit:**
- `current_value` мав **тихий fallback 1:1** при нечитабельному pool_state — міг повертати номінал без дохідності без жодного сигналу.
- Фікс: `read_nav` повертає `Nav { value, is_fallback }`; при fallback — `msg!`-лог + консервативна межа 1.0 (ніколи не завищує); `pool_state` пінниться `address`-constraint (`InvalidPoolState`).
- **Відкрите:** layout `pool_state` (`total_assets@8`, `total_supply@16`) **не верифіковано на мейннеті** — current_value може лягати на 1:1 fallback. Потребує перевірки реального layout і прибирання fallback. Див. SPEC.md §3.
