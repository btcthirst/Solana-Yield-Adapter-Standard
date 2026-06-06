# Fork-тестування: проблеми та розвʼязання

Post-mortem проблем, що спливли при прогоні mainnet-fork тестів (surfpool) проти реального стану. Раніше тести самоскіпались, тому ці баги не виявлялися — їх знайдено після фіксу L3 (CI падає без реального форку).

## Статус набору

| Adapter / suite | Стан | Документ |
|---|---|---|
| Maple | ✅ 4/4 (без fork-блокерів) | [dispatcher-and-maple](dispatcher-and-maple-fork-issues.md) |
| MarginFi | ✅ 4/4 (5 багів) | [marginfi](marginfi-fork-issues.md) |
| Kamino | ✅ 4/4 (V2 farms, 8 блокерів) | [kamino](kamino-fork-issues.md) |
| Jupiter LP | ✅ 4/4 (V2, 6 блокерів) | [jupiter-lp](jupiter-lp-fork-issues.md) |
| Dispatcher e2e | ✅ 8/8 | [dispatcher-and-maple](dispatcher-and-maple-fork-issues.md) |
| Drift IF | ❌ заблоковано | [drift](drift-fork-issues.md) |

**5 із 6 suite зелені (24 тести).** Drift — відкритий (deployed-програма розійшлася з публічним IDL для init-інструкцій).

## Класи проблем (повторювані по адаптерах)

1. **Застарілі дискримінатори** — deployed-програма на новій версії інструкцій: Kamino та Jupiter вимагали `*_v2`; Drift відхиляє навіть стандартний sha256 init-disc.
2. **Хибні byte-offset'и** — десеріалізація стану з чужих акаунтів: MarginFi (group 48→41, asset_share_value 88→80), Jupiter (aum_usd 180, без ratios). **Завжди звіряти offset з реальними даними акаунта, не з памʼяттю/коментарями.**
3. **Невідповідність моделі акаунтів** — MarginFi `marginfi_account` мусить бути keypair-signer (не PDA); writable/owner-constraints у Kamino/Jupiter (redeem на authority-ATA + переказ юзеру).
4. **Арг-кодування** — `Option<T>` у Borsh (MarginFi deposit/withdraw, Kamino/Jupiter V2 params).
5. **Oracle staleness** — Jupiter (Doves), Kamino (Scope) на форку.

## Наскрізні техніки роботи з surfpool (1.1.1)

- **Запуск:** `NO_DNA=1 surfpool start --rpc-url <RPC> --port 8899 --no-tui -y`. Прапори `--rpc-port`/`--network`+`--datasource-rpc-url` зі старого README/CI **застарілі** (потребують виправлення).
- **Не `--daemon`** усередині sandbox-виклику (процес гине при поверненні) — запускати як фоновий harness-таск.
- **Health** = JSON-RPC POST `getHealth`, НЕ GET `/health`. Чекати в логах `deployment' execution completed`, не лише health.
- **Авто-деплой** із `target/deploy/*.so`. Точковий апгрейд: `solana program deploy --program-id target/deploy/<p>-keypair.json target/deploy/<p>.so` (upgrade authority = локальний гаманець).
- **`anchor build` потребує `--ignore-keys`** (template і dispatcher мають target/deploy keypair'и, що не збігаються з declare_id; самі declare_id правильні).
- **Lazy-fetch** мейннет-акаунтів працює на читання; акаунти, що торкаються лише в CPI (vault-и, oracle, farm-стейти), часто треба клонувати явно в `00_setup` — **узгоджено за слотом** (інакше інваріанти балансів падають, як у Kamino).
- **Oracle staleness:** склонована ціна заморожується, а годинник surfpool іде вперед → `StaleOraclePrice`. Фікс — **підробити publish-timestamp вперед** через `surfnet_setAccount` перед tx (offset timestamp шукати діффом акаунта, прочитаного двічі). `surfnet_timeTravel` існує (`[{absoluteSlot|absoluteEpoch|absoluteTimestamp:N}]`) але **тільки вперед**; `--block-production-mode transaction` робить гірше; довгий сеанс заганяє годинник на ~1год вперед → рестарт скидає.
- **Знайти реальну транзакцію протоколу** (пагінація `getSignaturesForAddress` з `before`) — найнадійніший спосіб дізнатися точний порядок акаунтів і args, чого немає в IDL (remaining accounts) і коментарях.
