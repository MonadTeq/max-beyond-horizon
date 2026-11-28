# Дистрибутивы и графические установщики — план

Версия 0.1 (Архитектор) · 2026-11-28. Идея: ремейк устанавливается БЕЗ оригинальной игры (standalone), ресурсы в HD.

## Часть A. Инвентарь рантайм-зависимостей (по коду загрузчиков)

**Обязательный минимум:** `MAX.RES` (1828 записей, 17 МБ; входит в `data_hash` сети), `PATCHES.RES` (дескрипторы миссий/`SC_UNITS`), `COLOR.PAL`, карты `.WRL` (24, 28 МБ).

**Условно-обязательные:** звуки `*.SPW` (272, 24.5 МБ), портреты `*.FLC` (71, 9.5 МБ), видео `*.MVE` (3, 95.5 МБ), музыка `*.MSC` (13/16, 181.7 МБ; `BKG7/8/9` нет в GOG-копии — пропускаются штатно), миссии кампании (~11 МБ), `maxint.json` (субтитры).

**Вшито в бинарь:** шрифты FONT1/2/5, language/help/credits/юнит-таблицы.

**Не читается (исключить):** DOS-файлы, DOSBOX, `*.gog`, `MAX.INI`, MANUAL.PDF, SETUP.EXE и пр. (~12 МБ мусора из 378).

## Часть B. Раскладка (install read-only + user-data)

- **Windows:** `%ProgramFiles%\MAX-BRH\` (exe + SDL3.dll + `data\` + `remaster\` + лицензии); `%APPDATA%\MAX-BRH\` (saves/, missions/, preferences.ini, settings.jsonc).
- **Linux:** `/usr/lib/max-brh/` + `/usr/share/max-brh/data|remaster/` + `.desktop`; `$XDG_DATA_HOME/max-brh/` + `$XDG_CONFIG_HOME/max-brh/`.
- **Кодовые изменения этапа 2:** per-user дефолты сейвов, маркер данных `["max-original","data"]`, приоритет `settings.jsonc` (CLI → user-config → шаблон → дефолт).

## Часть C. HD-ассеты

**Отдельный опциональный пакет «HD Textures»** (Windows — компонент NSIS; Linux — пакет `max-brh-hd`). Полный банк ×2 ≈ 34 МиБ (ETC1S) / 136 МиБ (lossless). Поправить расхождение путей `default_remaster_root`.

## Часть D. Установщики

- **Windows — NSIS 3.x (MUI2) + MultiUser** (LZMA-solid, мастер/компоненты/Start Menu/ARP/uninstall/`/S`, RU+EN). WiX/MSI — только если понадобится AD/GPO.
- **Linux — deb+rpm (nfpm)** + **AppImage** (сборка на ubuntu:22.04 для совместимости; Steam Deck). Flatpak — отложить.
- Ассоциации файлов не создавать (v1).

## Часть E. Лицензии — флаги владельца (агенты не решают)

- **D1 (БЛОКЕР):** ассеты Interplay (MAX.RES-производные, © 1996) — standalone-распространение требует лицензии/соглашения, либо asset-import из копии пользователя, либо осознанный риск владельца.
- **D2:** HD-ассеты — производные + авторство новых работ.
- **D3:** `maxint.json` — транскрипция субтитров (производное).
- D4: подпись артефактов (Authenticode / GPG). D5: VC-рантайм (рекоменд. статическая CRT). D6: ProductName/AppId/publisher. D7: каналы доставки (сайт/itch/GOG/Steam). D8: Flatpak. D9: headless в инсталлятор (не рекоменд.). D10: сейвы при деинсталляции (оставлять + чекбокс). D11: локализация RU+EN. D12: MANUAL.PDF не включать.
- Код MIT; third-party (SDL3 zlib, ENet MIT, …) → `THIRD_PARTY_LICENSES` через cargo-about (гейт).

## Часть F. Этапы и гейты

| Этап | Содержание | Гейт |
|---|---|---|
| 0 | Лицензионный гейт (D1/D2/D3) | G0: решение владельца; профиль «standalone» или «asset-import wizard» |
| 1 | `asset_inventory` → `data-inventory.json` (sha256) | G1: манифест точный, детерминизм, оракулы чисты |
| 2 | Структура + user-data слой (код B4) | G2: дефолты, smoke read-only install, staging-макет 0 нехваток |
| 3 | NSIS-установщик (после G0) | G3: /S+uninstall на чистой VM, интерактивный проход, апгрейд поверх |
| 4 | deb/rpm + AppImage (после G0) | G4: чистые контейнеры debian/ubuntu/fedora + Steam Deck, lintian/rpmlint |
| 5 | CI `release.yml` по тегу v* | G5: все гейты зелёные, SHA256SUMS, публикация |

## Guardrails

Оракулы read-only; данные в пакете байт-в-байт = оракулы (иначе ломается `data_hash`/CRC); HD строго аддитивен; install-dir read-only в рантайме; детерминизм сборки; единый источник версии (workspace Cargo.toml).

## Риски

1. Лицензия данных (блокер D1). 2. Сдвиг дефолтов сейвов (аккуратные гейты, QA задаёт пути явно). 3. Медиа 277 МБ — возможен компонент «Music/Videos» отдельно.
