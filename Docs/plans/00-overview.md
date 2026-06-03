# План розробки: Solana Yield Adapter Standard

## Структура компонентів

```
Solana Yield Adapter Standard
├── 01-core-dispatcher      — Anchor-роутер, стандартний інтерфейс
├── 02-registry             — On-chain реєстр адаптерів з governance
├── adapters/
│   ├── 01-marginfi         — MarginFi USDC (найпростіший, старт)
│   ├── 02-drift            — Drift Insurance Fund
│   ├── 03-kamino           — Kamino USDC Vault
│   ├── 04-jupiter-lp       — Jupiter LP
│   └── 05-maple            — Maple Finance (найскладніший)
├── 03-mainnet-fork-tests   — Surfpool-based інтеграційні тести
└── 04-developer-docs       — Специфікація + "Build your own" гайд
```

---

## Послідовність розробки (рекомендована)

```
Тиждень 1–2:   Core Dispatcher (інтерфейс, конвенція, базова структура)
Тиждень 2–3:   MarginFi адаптер + перші тести
Тиждень 3–4:   Drift адаптер + Registry
Тиждень 4–6:   Kamino + Jupiter LP адаптери
Тиждень 6–8:   Maple адаптер (або Solend fallback)
Тиждень 8–10:  Mainnet-fork тести для всіх 5
Тиждень 10–11: Developer docs + специфікація
Тиждень 11–13: Review, bugfix, devnet deploy
Тиждень 13–17: Буфер (реалістична оцінка для 1 розробника)
```

> **Примітка щодо термінів:** При роботі одного Senior dev загальний обсяг складає ~84–89 робочих днів (~17–18 тижнів). 12 тижнів досяжні при паралельній роботі 2 розробників — один на Dispatcher + Registry + тести, другий на адаптери + docs.

---

## Залежності між компонентами

```
Core Dispatcher
    ↓
    ├── MarginFi Adapter (незалежний)
    ├── Drift Adapter (незалежний)
    ├── Kamino Adapter (незалежний)
    ├── Jupiter LP Adapter (незалежний)
    └── Maple Adapter (незалежний)

Registry
    └── залежить від: Core Dispatcher interface (frozen)

Mainnet-Fork Tests
    └── залежить від: всіх 5 адаптерів

Developer Docs
    └── залежить від: стабілізованого Core Dispatcher interface
```

---

## Критичний шлях

`Core Dispatcher interface` → `MarginFi` → `Registry` → `решта адаптерів` → `тести` → `docs`

Заморожування публічного інтерфейсу (`deposit / withdraw / current_value`) має відбутись **до** початку роботи над 3–5 адаптером, інакше буде rework.

---

## Ризики проекту

| Ризик | Ймовірність | Вплив | Мітигація |
|---|---|---|---|
| Maple не активний на Solana | Висока | Критичний | **Одразу замінити на Solend** — не витрачати час на реверс-інжиніринг |
| Jupiter LP IDL недоступний | Середня | Високий | Реверс-інжиніринг discriminators або REST API підхід |
| Mainnet-fork тести флакі | Висока | Середній | Seed фіксованих акаунтів, детермінований Clock state |
| Anchor 0.31.1 breaking changes | Низька | Середній | Локаут версій у Cargo.toml |
| Oracle stale у тестах | Середня | Середній | Surfpool Clock cheatcode для оновлення часу |
| CU перевищення у CPI-ланцюжках | Висока | Критичний | `ComputeBudgetProgram` у кожній інструкції (300k–400k CU) |

---

## Файли планів

- [Core Dispatcher](./01-core-dispatcher.md)
- [Registry](./02-registry.md)
- [MarginFi Adapter](./adapters/01-marginfi.md)
- [Drift Adapter](./adapters/02-drift.md)
- [Kamino Adapter](./adapters/03-kamino.md)
- [Jupiter LP Adapter](./adapters/04-jupiter-lp.md)
- [Maple Adapter](./adapters/05-maple.md)
- [Mainnet-Fork Tests](./03-mainnet-fork-tests.md)
- [Developer Docs](./04-developer-docs.md)

---

*Оновлено: 2026-06-03*
