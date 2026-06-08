# Drift Adapter — проблеми при mainnet-fork тестуванні

> 🛑 **ОСТАТОЧНИЙ ROOT CAUSE (2026-06-08): у задеплоєному Drift-проґрамі ВСІ інструкції закоментовані.**
> Найновіший коміт у репозиторії основної програми Drift буквально називається
> **«comment out all ixs»** — Drift свідомо вимкнув увесь набір інструкцій. Тому **будь-який**
> CPI у `dRiftyHA39…` повертає `AnchorError 101 (InstructionFallbackNotFound)`, **байт-у-байт як
> сміттєвий дискримінатор**. Це не «диски змінилися» (стара теорія нижче) — інструкцій просто
> немає. Підтверджено наживо (Helius mainnet, профінансований fee-payer):
> `initialize_user_stats` (`fef34862fb82a8d5`) → `Custom(101)`, 3374 CU **===** контроль
> `ffffffffffffffff` → `Custom(101)`, 3374 CU. Додатково: за ~88 годин — **0 прямих викликів**
> проґраму (фігурує лише як пасивний ALT/oracle-акаунт); канонічні акаунти (`state 5zpq…`,
> `usdc_spot_market 6gMq…`) досі належать `dRifty` → міграції на новий program ID **немає**.
>
> **Статус:** ЗАБЛОКОВАНО ЗОВНІШНЬО, **остаточно й непереборно з боку коду**. Жоден адаптер на
> raw-CPI (будь-який дискримінатор, IF чи spot-market) не пройде справжній fork-тест.
> `tests/fork/03_drift.ts` **коректно скіпиться** через `skipKnownBlocker` (probe доводить 101
> наживо → `ctx.skip()`, не падіння) — CI лишається зеленим, але тест НЕ зараховується як pass.
> Код адаптера лишаємо: він компілюється під сучасний стек і демонструє коректну CPI-модель
> (USDC spot-market lending), щойно Drift поверне інструкції — його легко ввімкнути.

> ℹ️ **Зміна моделі:** адаптер переписано з IF-стейкінгу на **USDC spot-market lending**
> (`initialize_user`/`deposit`/`withdraw`, market_index=0), бо IF-стейкінг вимкнено окремо
> (vault `2CqkQvYx…` замовк ~2026-04-02). Це нічого не змінює щодо блокера — spot-market
> інструкції лежать у тому самому закоментованому проґрамі й теж дають 101.

Програма Drift: `dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH` (адреса не змінювалась).

### 📌 Сорс-пруф (першоджерело — репозиторій Drift)

Перевірено напряму в `drift-labs/protocol-v2` (гілка `master`), 2026-06-08:

- **Коміт:** [`comment out all ixs` (#2174)](https://github.com/drift-labs/protocol-v2/pull/2174) —
  2026-04-01, тіло: *«comment out all ixs» + «fix: build errors no ixs»*. Це **остання змістовна
  зміна** `programs/drift` (після нього — лише бамп версії `v2.162.0`).
  Історія: <https://github.com/drift-labs/protocol-v2/commits/master/programs/drift>
- **Сам код:** [`programs/drift/src/lib.rs`](https://github.com/drift-labs/protocol-v2/blob/master/programs/drift/src/lib.rs)
  (2258 рядків) — **активна рівно ОДНА `pub fn`** (`program_entry`, кастомний entrypoint),
  **245 закоментованих `pub fn`** інструкцій (увесь `#[program]` вимкнено, напр. `// pub fn initialize_user...`).
- **Кастомний entrypoint** обслуговує лише 2 нативні oracle-операції під префіксом `FF FF FF FF`
  (`disc 0` → `handle_update_mm_oracle_native`, `disc 1` → `handle_update_amm_spread_adjustment_native`);
  усе інше падає в `else → entry()`, де anchor-диспетчер тепер не має жодної інструкції → `101`:

  ```rust
  if let [0xFF, 0xFF, 0xFF, 0xFF, discriminator, payload @ ..] = data {
      match *discriminator {
          0 => handle_update_mm_oracle_native(accounts, payload),
          1 => handle_update_amm_spread_adjustment_native(accounts, payload),
          _ => Err(ProgramError::InvalidInstructionData.into()),
      }
  } else {
      entry(program_id, accounts, data) // ← #[program] із 0 активних інструкцій → 101
  }
  ```

Це знімає будь-який сумнів: блокер — навмисне рішення Drift у сорсі, а не зміна дискримінаторів
чи артефакт surfpool/датасорсу. Заміряний `101` (== сміттєвий диск) — пряме слідство цього коду.

---

## ✅ ROOT CAUSE (доведено емпірично, 2026-06-06)

**Drift апгрейднув програму на мейннеті (слот `410633860`, ~2026-04-04) і змінив дискримінатори інструкцій. Жоден публічний IDL цього не відображає.**

Доказова база:

1. **`InstructionFallbackNotFound (101)`** — це Anchor-помилка «дискримінатор не збігся з жодною інструкцією». Логи CPI підтверджують: `BYT5… invoke [1] → dRifty… invoke [2] → AnchorError 101`.
2. **Усі задокументовані sha256-дискримінатори відкидаються поточною програмою** — перевірено `simulateTransaction` проти **живого мейннету (Helius)** з профінансованим payer:
   - `add_insurance_fund_stake` `fb90730bde2f3eec` → 101
   - `request_remove_insurance_fund_stake` `8e46cc5c496ab434` → 101
   - `initialize_user_stats` `fef34862fb82a8d5` → 101
3. **Вирішальний replay:** взято **реальну** дотранзакцію `add_insurance_fund_stake` (слот `410363203`, 2026-04-01, 11 акаунтів, data `fb90730b…0000e803000000000000`) і відтворено байт-у-байт проти **поточної** програми → **той самий 101**. Тобто інструкція, що працювала 1 квітня, тепер відкидається.
4. **Апгрейд-слот `410633860` > слот тієї tx `410363203`** — програму змінили ПІСЛЯ тих успішних транзакцій. Усі знайдені IF-tx із sha256-дисками датовані до 1 квітня (до апгрейду).
5. **Усі публічні IDL застарілі / pre-upgrade:**
   - on-chain IDL акаунт = `2.150.0`, старий формат (без явних `discriminator`), не оновлювався при апгрейді;
   - npm `@drift-labs/sdk` `2.162.0` і `2.163.0-beta.13` — старий формат / явні диски = старі sha256; адреса в IDL `vELoC1…` = це **devnet** ID (`DRIFT_DEVNET_PROGRAM_ID`), мейннет константа в SDK досі `dRifty…`;
   - `driftpy` `2.142.0`, `drift-rs` — теж старі;
   - `protocol-v2` HEAD досі має `add_insurance_fund_stake` зі стандартним ім'ям → задеплоєна v3 **зібрана НЕ з публічного сорсу**.
6. **Це НЕ артефакт surfpool/датасорсу:** ProgramData форку байт-ідентичний мейннету (`7dLgmtcT…`, 6 673 114 байт, sha `a73b7243…`); живий мейннет поводиться так само, як форк.

**Висновок:** дискримінатори у `cpi.rs` (`DISC_*`) — старі (v2) і не диспетчеризуються поточною програмою. Розблокування потребує **дискримінаторів v3**, яких нема в жодному публічному джерелі.

### Додатково: Drift v3 = «Drift Safety Module» (DSM)

За docs.drift.trade, v3 ввів **DSM**: стейкають **DRIFT-токен** в ізольовані per-asset пули IF, а не USDC у market 0. Тобто змінилась і **модель** IF-стейкінгу, не лише диски — підхід адаптера (USDC, market_index=0) може взагалі не мапитись на v3 без переробки.

---

## ✅ Що ВЖЕ виправлено в коді (перевірено проти v2 IDL + layout-ів)

Це реальні баги, незалежні від блокера дискримінаторів; матимуть значення, щойно з'являться v3-диски:

- **Порядок акаунтів усіх CPI** приведено до Drift IDL v2.162:
  - `initialize_insurance_fund_stake`: `state` тепер перед `authority`/`payer`.
  - `add_insurance_fund_stake`: додано відсутній `drift_signer`; `insurance_fund_vault` поставлено на правильну позицію (10 акаунтів).
  - `request_remove_insurance_fund_stake`: прибрано фантомний `state`, **додано `insurance_fund_vault` і arg `amount: u64`** (це USDC-сума, яку Drift конвертує в shares через `vault_amount_to_if_shares`).
  - `remove_insurance_fund_stake`: `insurance_fund_vault` переміщено на позицію 5.
- **`Deposit` контекст**: додано акаунт `drift_signer` (+ у тест `03_drift.ts`).
- **`withdraw` step 1**: рахує `amount` = повна USDC-вартість позиції (`if_shares * vault_amount / total_shares`, floor) і передає в `request_remove`.
- **Байт-офсети звірено з живими даними:** `total_shares`=336, `unstaking_period`=384 (=1123200с=13 днів ✓), `if_shares`=40, IF vault `2CqkQvYx…` не змінився. (Виправлено лише мертвий reader `last_withdraw_request_ts`: 96→104.)

---

## Що потрібно для розблокування

1. **IDL програми Drift v3** з явними дискримінаторами (немає публічно). Спитати в Drift Discord `#research-and-dev-chat`, або в команди баунті.
2. Або **витягти диски з пост-апгрейд (слот > 410633860) транзакції**, що реально *викликає* `dRifty` IF-інструкції. Проблема: `getSignaturesForAddress` (навіть Helius) повертає переважно tx, які лише *читають* Drift-акаунти (оракул), а не викликають програму; IF-стейкінг малоактивний. Потрібен надійніший індекс (Drift Data API) або відома пост-апгрейд IF-tx.
3. Окремо з'ясувати, чи v3/DSM взагалі підтримує USDC-IF-стейкінг у тій формі, що реалізує адаптер.

---

## Корисний контекст

- Drift: `dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH`; state `5zpq7DvB…`; USDC spot market `6gMq3mRC…`; USDC IF vault `2CqkQvYx…` (актуальний, звірено з `spot_market.insurance_fund.vault`).
- ✅ Адаптерна константа `DRIFT_SIGNER = JCNCMFXo5M5qwUPg2Utu1u6YWp3MbygxqBsBeXXJfrw` — **ВЕРИФІКОВАНО наживо (2026-06-08, Helius):** дорівнює `State.signer` (offset 104 у State-акаунті `5zpq7DvB…`) і є канонічним PDA `[b"drift_signer"] @ DRIFT_PROGRAM_ID`. Константа правильна (попередній сумнів про «адресу Drift Vaults» знято).
- ✅ Решта констант звірені з мейннетом (2026-06-08): `spot_market_vault` PDA `[b"spot_market_vault", 0u16] = GXWqPpjQpdz7KZw9p7f5PX2eGxHAhvpNXiviFkAB8zXg` (mint = USDC; == `SpotMarket.vault` @104 у `6gMq3mRC…`); USDC spot market `.mint` = USDC, `.pubkey` self-consistent; `DRIFT_STATE` owned by Drift. Усі account/PDA/program константи коректні — блокером лишається ТІЛЬКИ gutted-програма (101), не дані. Диски `sha256("global:<name>")` = опублікований IDL (проти задеплоєної програми незвірювані, бо всі ix закоментовані).
- Запуск форку (потрібен робочий mainnet RPC, публічний api.mainnet-beta не годиться — surfpool падає; publicnode не віддає декодованих tx; робить Helius):
  `NO_DNA=1 SURFPOOL_DATASOURCE_RPC_URL=<helius> surfpool start --no-tui -y --port 8899`
