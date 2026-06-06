# Drift Insurance Fund Adapter — проблеми при mainnet-fork тестуванні

> Статус: **НЕ ВИРІШЕНО** (заблоковано). `tests/fork/03_drift.ts` червоний — падає на першому ж CPI у Drift.
> У код адаптера зміни НЕ вносились (лише дослідження). Це найглибший і найбільш невизначений блокер із усіх адаптерів.

Програма Drift v2: `dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH`
(підтверджено: володіє USDC spot market `6gMq3mRC…` — тобто program ID в адаптері **правильний**; варіант `…ozatg` з памʼяті був хибний/відсутній на мейннеті).

---

## Головний блокер: `InstructionFallbackNotFound` (101) на `initialize_user_stats`

**Симптом:** `initialize_position` → перший CPI у Drift (`initialize_user_stats`) одразу падає з `InstructionFallbackNotFound` («Fallback functions are not supported»). У Anchor це означає **дискримінатор не збігся з жодною інструкцією**.

### Що перевірено (усе вказує, що disc МАЄ бути правильним)

1. **Дискримінатор = стандартний sha256.** `fef34862fb82a8d5 = sha256("global:initialize_user_stats")[..8]` — збігається з хардкодом адаптера.
2. **Інші Drift-диски адаптера правильні й підтверджені на реальних мейннет-tx:**
   - `add_insurance_fund_stake` = `fb90730bde2f3eec` ✓ (бачили в реальних tx)
   - `request_remove_insurance_fund_stake` = `8e46cc5c496ab434` ✓
   - `settle_revenue_to_insurance_fund` = `c8785d…` ✓
   Тобто Drift використовує **стандартну anchor-схему** `sha256("global:<snake>")`.
3. **Публічний IDL/SDK (`@drift-labs/sdk` v2.156–2.163)** містить `initializeUserStats` (snake → `initialize_user_stats` → той самий fef348).
4. **Байткод Drift на surfpool байт-ідентичний мейннету** (ProgramData `7dLgmtcT…`, 6 673 114 байтів, sha256 кількох зрізів збігається). Тобто це НЕ артефакт форку і НЕ стале завантаження.

### Вирішальний експеримент

Надіслав **сирий** `initialize_user_stats` (disc `fef348`, без args, 6 акаунтів за IDL) **напряму** deployed-Drift на форку → **той самий `InstructionFallbackNotFound`**.

Оскільки байткод = мейннет, мейннет поводився б так само.

### Висновок

**Deployed mainnet-Drift розійшовся з публічним IDL/SDK саме для init-інструкцій.** Часті інструкції (`add`/`request_remove`/`settle`) — стандартний sha256 і працюють; init-інструкції (`initialize_user_stats`, ймовірно й `initialize_insurance_fund_stake`) — мають **інший дискримінатор**, ніж документований. Це означає: instruction перейменовано/рефакторено у версії, що задеплоєна, але публічний IDL цього не відображає.

### Що потрібно для розблокування

Реальні дискримінатори init-інструкцій **deployed-Drift**. Їх немає:
- on-chain (Drift не публікує anchor-IDL: `fetchIdl` → null),
- у публічному github/SDK у відповідній формі (старий формат IDL без явних discriminator-полів).

Єдиний шлях — **реверс із реальної транзакції онбордингу нового юзера** (вона викликає `initialize_user_stats`/`initialize_user`). Спроби знайти таку tx пагінацією/скануванням стейт-акаунта **не вдалися** через парсинг versioned-tx (jsonParsed не витягував Drift-інструкції зі стейт-акаунта). Альтернатива — `drift-rs` (Rust SDK, `drift-labs/drift-rs`), який може мати актуальні визначення для CPI-інтеграції.

---

## Додаткові проблеми Drift (виявлені попутно, теж потребуватимуть фіксу)

### 1. Хибний порядок акаунтів у `initialize_insurance_fund_stake`

Публічний IDL: `[spotMarket, insuranceFundStake, userStats, state, authority, payer, rent, systemProgram]`.
Адаптер (`cpi.rs`): `… userStats, authority, payer, state …` — **`state`/`authority`/`payer` переплутані**. Це не `InstructionFallbackNotFound` (та помилка disc-only), але спливе після розблокування головного блокера.

### 2. 2-крокова cooldown-withdraw (~13 днів)

`withdraw` = `request_remove_insurance_fund_stake` → cooldown → `remove_insurance_fund_stake`. Тест «withdraw step2» потребує **перемотування годинника surfpool** (`surfnet_timeTravel [{absoluteTimestamp: now + cooldown}]` — рухає лише вперед, що тут якраз підходить).

### 3. `DRIFT_SIGNER` неперевірений

Константа в адаптері `JCNCMFXo5M5qwUPg2Utu1u6YWp3MbygxqBsBeXXJfrw` — не звірена з мейннетом.

---

## Корисний контекст для відновлення роботи

- Реальний Drift: `dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH`; state `5zpq7DvB…`; USDC spot market `6gMq3mRC…`; USDC IF vault `2CqkQvYx…`.
- Підтверджені диски (sha256): add=`fb90730bde2f3eec`, request_remove=`8e46cc5c496ab434`, settle=`c8785d884526c79f`.
- Невідомі диски, що зустрічались навколо IF-vault (можливі кандидати на перейменовані інструкції): `7cc2f0fec6d5347a`, `e4d0bff6a93abdd5`, `e010b0d6a2d5b7de`.
- Drift docs: docs.drift.trade/developers → SDK (TS `@drift-labs/sdk`, Python `driftpy`, Rust `drift-rs`).

## Порівняння з Jupiter

Jupiter мав схожий симптом (V1 disc відхилявся), але там disc просто змінився на V2-варіант (`...2`), а структуру вдалося відновити з реальної tx. Для Drift справжній disc init-інструкцій поки **не знайдено** — це і є відмінність, через яку Drift лишається відкритим.
