# Kamino USDC Adapter — проблеми при mainnet-fork тестуванні

> Статус: **ВИРІШЕНО** — `tests/fork/02_kamino.ts` зелений 4/4. Commits: `0dc2715` (частково), `19d8043` (V2 farms).
> Найважча DeFi-інтеграція: **8 послідовних блокерів**. Усе звірено з on-chain klend IDL і даними резерву.

KLend програма: `KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD`
USDC reserve: `D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59`
Farms програма: `FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr`

---

## 1. `InvalidScopePriceAccount` (6056) у `refresh_reserve`

**Корінь:** резерв сконфігурований на **Scope**-оракул, а тест передавав placeholder. Потрібен конкретний scope price feed резерву: `3t4JZcueEzTbVP6kLxXrL3VpWx45jDer4eqysweBchNH` (знайдено сканом config-зони резерву на акаунт, що належить scope-програмі).

**Фікс:** передавати scope як `refresh_reserve` oracle-слот **[3]**; слоти [0..2] = klend program id (сентинел «None»).

---

## 2. `writable privilege escalated` на `kamino_authority`

**Корінь:** Kamino deposit/withdraw вимагають `owner` (obligation owner) **writable**, а struct адаптера мав `kamino_authority` без `mut`.

**Фікс:** додати `mut` до `kamino_authority` у deposit і withdraw.

---

## 3. ⭐ `CpiDisabled` (6080) — farmed reserve потребує V2

**Корінь:** USDC-резерв головного ринку має **активну farm** (`farmCollateral = JAvnB9AKtgPsTEoKmn24Bq64UMoYcrtWtq42HHBdsPkh`). V1 `deposit_reserve_liquidity_and_obligation_collateral` через CPI на farmed-резерві відхиляється з `CpiDisabled` у `refresh_ix_utils.rs`.

**Фікс:** перейти на **V2**-інструкції з farm-акаунтами:
- `deposit_reserve_liquidity_and_obligation_collateral_v2` (disc `d8e0bf1bcc9766af`)
- `withdraw_obligation_collateral_and_redeem_reserve_collateral_v2` (disc `eb34779895c51407`)
- V2 несе `obligation_farm_user_state`, `reserve_farm_state`, `farms_program`.
- При init додати `init_obligation_farms_for_reserve` (mode 0 = collateral). `obligation_farm` PDA = `[b"user", reserve_farm_state, obligation]` @ farms program.

Референс — інтеграція marginfi-v2 (`programs/marginfi/src/instructions/kamino/`).

---

## 4. `ObligationStale` (6017) у V2-deposit

**Корінь:** V2-deposit вимагає **свіжий obligation**. Адаптерів deposit робив лише `refresh_reserve`.

**Фікс:** додати `refresh_obligation` між refresh_reserve і deposit.

---

## 5. `InvalidAccountInput` у `refresh_obligation` на порожньому obligation

**Корінь:** Kamino `refresh_obligation` робить `zip_exact` активних депозитів із переданими резервами. На першому депозиті obligation порожній (0 депозитів) → передача `[reserve]` (1) ламає zip_exact.

**Фікс:** передавати **0 резервів коли `shares == 0`** (порожній obligation), `[reserve]` інакше. Критерій для single-reserve адаптера: `shares == 0` ⟺ немає USDC-депозиту (Kamino прибирає порожні депозити при full-withdraw).

---

## 6. `InvalidAccountInput@lending_operations.rs:1581` — резерви мають бути writable

**Корінь:** deployed-klend `refresh_obligation` вантажить передані резерви **mutably**; адаптер передавав їх readonly → load fail → `InvalidAccountInput`.

**Фікс:** у `cpi_refresh_obligation` передавати резерви як `AccountMeta::new` (writable).

---

## 7. ⭐ `subtract overflow@lending_checks.rs:462` — неузгоджені баланси vault

**Симптом:** deposit доходить до MintTo (cToken'и карбуються), потім паніка subtract-overflow у post-deposit інваріанті.

**Корінь:** Kamino перевіряє, що баланси vault-ів збігаються з трекнутими `reserve.available_amount`/`mint_total_supply`. На форку резерв клонувався на слоті T1, а його vault'и lazy-fetch на іншому слоті → розбіжність → underflow.

**Фікс:** клонувати в `00_setup` **разом** із резервом його vault-и (liquidity supply, collateral mint, collateral supply) — щоб баланси були з одного снапшоту.

---

## 8. `ConstraintTokenOwner` (2015) у withdraw

**Корінь:** Kamino V2-withdraw вимагає, щоб destination USDC належав obligation-owner'у (`kamino_authority`), а не юзеру.

**Фікс:** redeem USDC на authority-USDC-ATA, потім `invoke_signed` SPL-transfer дельти юзеру.

---

## Висновок

Kamino — приклад протоколу з багатошаровою валідацією (farms, scope-oracle, інваріанти балансів, owner-constraints). Метод: фіксити по одному блокеру, **звіряючись з IDL і реальною інтеграцією (marginfi-v2)**, і клонувати всі CPI-дотичні акаунти узгоджено за слотом.
