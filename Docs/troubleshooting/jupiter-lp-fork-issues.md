# Jupiter LP Adapter — проблеми при mainnet-fork тестуванні

> Статус: **ВИРІШЕНО** — `tests/fork/04_jupiter_lp.ts` зелений 4/4 (init, deposit, current_value, withdraw).
> Commit: `dd4764f`. Усе звірено з on-chain IDL Jupiter Perps та реальною `addLiquidity2`-транзакцією.

Програма Jupiter Perpetuals: `PERPHjGBqRHArX4DySjwM6UJHiR3sWAatqfdBS2qQJu`
JLP pool: `5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq` (правильна, не плейсхолдер).

Адаптер впирався в **6 послідовних проблем** — кожна виявлялася лише після виправлення попередньої.

---

## 1. V1-інструкції відхиляються (`InstructionFallbackNotFound`, 101)

**Симптом:** `deposit` → CPI у Jupiter падає з `InstructionFallbackNotFound`.

**Корінь:** адаптер використовував V1-дискримінатори `add_liquidity`/`remove_liquidity`, але deployed-програма приймає лише **V2**: `addLiquidity2` / `removeLiquidity2`.

**Фікс:**
- `DISC_ADD_LIQUIDITY = e4a24e1c46db7473` (sha256 `global:add_liquidity2`)
- `DISC_REMOVE_LIQUIDITY = e6d7527ff165e392` (sha256 `global:remove_liquidity2`)

---

## 2. Інша структура акаунтів V2

V2 має інший список іменованих акаунтів (звірено з IDL):

```
0  owner (signer, НЕ mut)         7  custodyDovesPriceAccount (ro)
1  fundingAccount (mut)           8  custodyPythnetPriceAccount (ro)
2  lpTokenAccount (mut)           9  custodyTokenAccount (mut)
3  transferAuthority (ro)        10  lpTokenMint (mut)
4  perpetuals (ro!)              11  tokenProgram
5  pool (mut)                    12  eventAuthority [b"__event_authority"]
6  custody (mut)                 13  program (PERP — self-ref для emit_cpi)
```

Ключові відмінності від V1: `owner` тепер **signer але не mut**; `perpetuals` **readonly**; **два** цінові акаунти замість одного; додано `eventAuthority` + `program`.

Args `addLiquidity2`: `token_amount_in: u64, min_lp_amount_out: u64, token_amount_pre_swap: Option<u64>` (передаємо `None` = `0x00`).

---

## 3. AUM-розрахунок потребує всі custody (`NotEnoughAccountKeys`)

**Симптом:** після виправлення disc — `AddLiquidity2` доходить до «Compute assets under management» і падає з `NotEnoughAccountKeys`.

**Корінь:** щоб порахувати AUM усього пулу, Jupiter читає **всі 5 custody + їхні ціни** як *remaining accounts*.

**Як знайшли точну структуру:** реальних `addLiquidity2`-tx у свіжих блоках мало — знайшли **пагінацією** (`getSignaturesForAddress` з `before`, ~3 сторінки) і здампували повний список акаунтів. Виявилось:

```
remaining = [custody0, custody1, custody2, custody3, custody4,
             pythnet0, pythnet1, pythnet2, pythnet3, pythnet4]
```
тобто **спочатку всі 5 custody, потім усі 5 pythnet-цін**, у порядку `pool.custodies`. НЕ тріплети `[custody, doves, pythnet]`, як здавалося спершу (це давало `Left/Right` mismatch).

**Несподіванка:** іменовані слоти 7 і 8 (`custodyDoves`/`custodyPythnet`) для USDC **обидва = той самий pythnet-акаунт** (offset 384 у custody), а не doves(320)+pythnet(384).

Custody-акаунти й оракули читаються з `pool.custodies` (offset після `name`) та з кожного custody (doves@320, pythnet@384).

---

## 4. ⭐ `StaleOraclePrice` (6003) — головна fork-проблема

**Симптом:** `oracle.rs:265 StaleOraclePrice`, `Left: 5, Right: 14..37`.

**Корінь:** Jupiter відхиляє ціни старші за **~5 секунд**. На форку склонований оракул «заморожений», а годинник surfpool іде вперед → вік ціни швидко перевищує 5с. Перекдонування з мейннету не допомагає (інтринсік-лаг оракула + latency tx ≈ 14с > 5с).

**Що НЕ спрацювало:**
- `surfnet_timeTravel` — існує (`[{absoluteSlot|absoluteEpoch|absoluteTimestamp: N}]`), але рухає годинник **тільки вперед**; назад (до слота оракула) не можна.
- `--block-production-mode transaction` — зробило **гірше** (годинник виставляється інакше, age=616).
- Рестарт surfpool скидає дрейф (довгий сеанс заганяє годинник ~1год вперед), але age все одно ~14.

**✅ Фікс (універсальна техніка для Pyth/Doves на форку):** **підробити publish-timestamp оракула вперед** через `surfnet_setAccount` безпосередньо перед інструкцією:
1. Прочитати `Clock.unix_timestamp` (i64 @ offset 32 sysvar `SysvarC1ock…`).
2. Для кожного pythnet-акаунта: fetch із мейннету → перезаписати **i64 @ offset 177** на `clock + 60` → `surfnet_setAccount`.
3. Виконати tx негайно.

Тепер `current_time - publish_time < 0 ≤ max_age` → ціна «свіжа». Поле timestamp знайдено **діффом** того самого оракула, прочитаного двічі з інтервалом (змінні байти = price/conf/timestamp). Див. `refreshFromMainnet()` у тесті.

---

## 5. `withdraw` → `ConstraintHasOne` (2001)

**Корінь:** Jupiter вимагає, щоб `receivingAccount` (куди йде USDC) **належав власнику позиції** (`jlp_authority` PDA), а не юзеру.

**Фікс (як у Kamino):** redeem USDC на **authority-USDC-ATA**, потім `invoke_signed` SPL-transfer дельти на ATA юзера.

---

## 6. `current_value` повертав 0 (хибний offset `aum_usd`)

**Корінь:** `read_pool_aum` припускав layout `name + custodies + ratios + aum`, але `ratios`-вектора немає — `aum_usd` (u128) йде **одразу після `custodies`** (offset **180** на мейннеті). Зайвий skip читав сміттєвий `ratios_count` і збивав offset → aum=0 → помилка.

**Фікс:** прибрати skip `ratios`; читати `aum_usd` одразу після custodies. Перевірка: aum=699366319068734 → ціна $3.15/JLP (осмислено).

---

## Корисні константи (mainnet, перевірені)

| Що | Адреса |
|---|---|
| Pool custodies | `7xS2…1wdz`, `AQCG…rEjEn`, `5Pv3…ALkm`, `G18j…46EZa`(USDC), `4vkN…6ZETkk` |
| pythnet-ціни (offset 384) | `FYq2…5zXh`, `AFZn…tfXF1`, `hUqA…fRfmC`, `6Jp2…fjnM`(USDC), `Fgc9…QbF5u` |
| eventAuthority | `37hJBDnntwqhGbK7L6M1bLyvccj4u55CCUiLPdYkiqBN` |

## Висновок-метод

Найцінніше: **знайти реальну транзакцію цільового протоколу** (пагінацією по сигнатурах) — вона дає точний порядок акаунтів і args, чого немає в IDL (remaining accounts) і в коментарях. Для оракул-staleness — **forge timestamp**, бо `timeTravel` лише вперед.
