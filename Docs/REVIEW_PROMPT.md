# Промт: аудит відповідності та якості проєкту Solana Yield Adapter Standard

> Готовий промт для рев'ю репозиторію в новій сесії Claude Code / іншому агенті-рев'юері.
> Перевіряє відповідність вимогам баунті (`Docs/Tasks/GENRAL_IDEA.md`) та дотримання
> найкращих практик безпеки, архітектури, тестування й документації.

---

Ти — досвідчений Solana/Anchor аудитор. Проведи повний review цього репозиторію.
Мета: (1) перевірити відповідність вимогам баунті у `Docs/Tasks/GENRAL_IDEA.md`,
(2) оцінити дотримання найкращих практик безпеки, архітектури, тестування й документації.

## Правила роботи
- Перевіряй ФАКТАМИ з коду, а не з документації проєкту. Якщо README/SPEC щось
  стверджує — відкрий відповідний файл і підтверди, що це справді так.
- Для кожної знахідки вказуй `file_path:line` і severity: 🔴 Critical / 🟠 High /
  🟡 Medium / 🔵 Low / ℹ️ Info.
- Не вигадуй вимог поза GENRAL_IDEA.md. Розділяй «вимога баунті» та «best practice».
- Якщо щось не можеш перевірити (напр. mainnet-fork без RPC) — позначай як
  "не верифіковано" і поясни, що потрібно для перевірки.

## Частина 1 — Відповідність GENRAL_IDEA.md
Звірся з пунктами Scope та Submission Requirements. Для кожного — статус
✅ / ⚠️ / ❌ і докази:

1. **Core Dispatcher** — Anchor-програма-роутер зі стандартним інтерфейсом
   `deposit`, `withdraw`, `current_value`. Перевір `programs/dispatcher/`:
   чи реалізовані всі три, чи інтерфейс справді стандартизований і generic
   (не захардкоджений під один протокол).
2. **П'ять референсних адаптерів** — Kamino USDC, MarginFi USDC, Jupiter LP,
   Maple Syrup, Drift Insurance Fund. Перевір, що всі 5 існують у `programs/*`,
   кожен має 4 інструкції й реальну CPI-логіку (не заглушки/TODO).
3. **On-Chain Registry** — governance-gated механізм реєстрації адаптерів.
   Перевір `programs/registry/`: чи approve/revoke/pause захищені authority,
   чи є двоетапна передача authority, чи Dispatcher справді звіряється з реєстром
   перед CPI.
4. **Mainnet-fork тести** — інтеграційні тести для всіх 5 адаптерів проти
   mainnet-стану. Перевір `tests/`: чи клонується реальний mainnet-стан,
   чи покривають happy-path + негативні кейси, чи всі 5 адаптерів охоплені.
5. **Specification** (`SPEC.md`) — стандарт адаптера у markdown. Перевір повноту,
   точність кодів помилок та сигнатур відносно коду.
6. **"Build your own adapter" guide** (`ADAPTER_GUIDE.md`) — чи реально дозволяє
   зробити робочий адаптер за день: чи є template (`programs/template/`),
   покрокові інструкції, реєстрація, тестування.
7. **Deploy на devnet** — Registry задеплоєний на devnet. Звір program ID у
   `Anchor.toml` / деплой-нотатках, познач що верифікується тільки в мережі.
8. **Tech stack** — GENRAL_IDEA вказує Anchor 0.31.1 / Solana 2.2.20, але
   організатори баунті в коментарях до завдання офіційно дозволили
   використовувати ОСТАННІ версії Anchor/Solana. Проєкт на Anchor v1 /
   Solana 3.x — це ✅ дозволено, НЕ трактуй як розходження чи ризик.
   Натомість перевір, що проєкт коректно й без warning'ів збирається на
   заявлених версіях, версії узгоджені між усіма `Cargo.toml`/`Anchor.toml`,
   і ніде в коді/доках немає протиріч (напр. README обіцяє 0.31, а збірка на v1).

## Частина 2 — Judging criteria (оціни кожен у %)
- **Correctness (40%)** — чи адаптери коректно працюють проти mainnet-fork.
- **Interface Design (25%)** — чистота, мінімальність, розширюваність стандарту.
- **Developer Guide (20%)** — наскільки легко новій команді зробити адаптер за день.
- **Code Quality & Test Coverage (15%)** — загальна якість і повнота тестів.
Дай орієнтовну оцінку по кожному і що саме знижує бал.

## Частина 3 — Best practices (безпека Solana/Anchor)
- **Account validation**: усі account constraints (`has_one`, `seeds`, `bump`,
  `owner`, `address`), відсутність невалідованих `AccountInfo`/`UncheckedAccount`
  без `/// CHECK`, перевірка owner і program ID при raw CPI (`invoke_signed`).
- **PDA & signer**: коректність seeds, відсутність signer-seed leakage,
  чи правильно пропагуються підписи через Dispatcher → adapter → protocol.
- **Arithmetic**: усі обчислення shares/value через checked-математику
  (особливо I80F48, обмінні курси, масштабування decimals) — без overflow/
  precision loss; перевір читання byte-offset'ів з чужих акаунтів на крихкість.
- **CPI safety**: при raw CPI з захардкодженими дискримінаторами — чи звіряється
  цільовий program ID; чи валідуються remaining_accounts (оракули тощо).
- **Authorization**: registry-інструкції лише для authority; стан paused/revoked
  справді блокує deposit/withdraw у Dispatcher.
- **Return data**: `set_return_data`/`get_return_data` — межі розміру, узгодженість
  серіалізації між адаптером і Dispatcher.
- Прогони `program_autofixer` (Solana MCP) по кожній програмі й включи висновки.

## Частина 4 — Якість коду й репозиторій
- Узгодженість кодів помилок між `error.rs`, SPEC.md і тестами.
- Дублювання між адаптерами — чи варто винести спільне (cpi_utils, shares-math).
- `Cargo.toml`/`Anchor.toml`: версії, features, прибрані зайві залежності.
- CI (`.github/workflows/test.yml`): чи реально ганяє build + fork-тести.
- Гігієна репо: відсутність закомічених `target/`, ключів, секретів;
  `.gitignore` коректний.
- Документація: README як точка входу, нема «битих» посилань і застарілих
  program ID.

## Формат звіту
1. **Executive summary** — 5–8 рядків: чи готовий проєкт до здачі, головні ризики.
2. **Таблиця відповідності** GENRAL_IDEA (вимога → статус → докази).
3. **Оцінка за judging criteria** з %.
4. **Знахідки** згруповані за severity, кожна з `file:line` і рекомендацією фіксу.
5. **Топ-N пріоритетних дій** перед здачею (відсортовано за впливом на оцінку).
