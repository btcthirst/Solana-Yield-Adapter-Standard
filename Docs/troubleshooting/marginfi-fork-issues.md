# MarginFi USDC Adapter — проблеми при mainnet-fork тестуванні

> Статус: **ВИРІШЕНО** — `tests/fork/01_marginfi.ts` зелений 4/4. Commit: `8978647`.
> Прогін проти реального форку (раніше тести самоскіпались) виявив **5 реальних багів** — усі звірені з on-chain IDL MarginFi та даними банку.

MarginFi програма: `MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA`
USDC bank: `2s37akK2eyBbp8DZgCm7RtsaEz8eJP3Nxd4urLHQv7yB`

---

## 1. Хибний offset групи в тесті (`AccountOwnedByWrongProgram`, 3007)

**Корінь:** тест читав `marginfi_group` з банку за offset **48**, але насправді група — на offset **41**. Offset 48 давав сміттєву адресу, якої немає → на форку трактувалась як порожній System-акаунт.

**Bank layout (звірено):** `disc(8) + mint(32)@8 + mint_decimals(1)@40 + group(32)@41`.

**Фікс:** читати групу на `41..73` (у `01_marginfi.ts` і `06_dispatcher.ts`).

---

## 2. `marginfi_account` як PDA замість Signer (`signer privilege escalated`)

**Корінь:** адаптер деривував `marginfi_account` як PDA, але MarginFi `marginfi_account_initialize` вимагає, щоб це був **keypair-signer** (IDL: account #1 `signer=true`, `init` без seeds). На мейннеті всі marginfi-акаунти — випадкові keypair'и.

**Фікс:** у `initialize_position` зробити `marginfi_account: Signer<'info>`; CPI позначає його `signer=true`; клієнт генерує keypair і підписує init-tx.

---

## 3. `deposit` — бракує байта `Option<bool>`

**Корінь:** IDL `lending_account_deposit` args: `amount: u64, deposit_up_to_limit: Option<bool>`. Адаптер слав лише `amount` → провал десеріалізації.

**Фікс:** дописати `0x00` (None) після amount.

---

## 4. `withdraw` — хибне кодування `Option<bool>`

**Корінь:** IDL args: `amount: u64, withdraw_all: Option<bool>`. Адаптер слав «голий» байт `withdraw_all as u8`.

**Фікс:** кодувати Option: `Some(true) = [0x01, 0x01]`, `None = [0x00]`.

---

## 5. ⭐ Хибний offset `asset_share_value` → shares ≈ 0

**Симптом:** deposit «проходить» (USDC рухається), але `pos.shares == 0`; current_value = 0.

**Корінь:** адаптер читав `asset_share_value` (I80F48) за offset **88**, а реально воно на **80**. Offset 88 потрапляв між asset- і liability-значеннями → сміття → `amount * 2^48 / garbage ≈ 0`.

**Bank layout (перевірено емпірично сканом):** `group@41..73, _pad@73..80, asset_share_value@80..96, liability_share_value@96..112`.

**Фікс:** `ASSET_SHARE_VALUE_OFFSET = 80`. Перевірка: ratio 1.2246 (>1.0 від накопичених відсотків) — осмислено.

---

## Висновок

MarginFi виявився «еталоном» того, як приховані баги десеріалізації (offset'и) та невідповідність моделі акаунтів (PDA vs keypair-signer) спливають лише при реальному прогоні. Усі offset'и треба **звіряти з даними акаунта**, не довіряти памʼяті/коментарям.
