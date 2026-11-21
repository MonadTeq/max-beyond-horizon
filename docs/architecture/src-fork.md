# src/ — наша развилка C++-порта (headless-сервер)

## Отношение к первоисточнику

- **Первоисточник**: `max-cpp-port/` (klei1984/max, git-субмодуль) — **не изменяется**,
  используется только как референс.
- **`src/`** — развилка первоисточника с нашими доработками для роли
  headless-сервера под Horizon. Правило: сначала смотреть, как сделано в
  `max-cpp-port/`; наши изменения — только серверная интеграция.
- Проверить дельту: `git diff --no-index max-cpp-port/src src` (Windows CRLF даёт
  шум — сравнивать с `--ignore-cr-at-eol` при необходимости).

## Наши добавления (файлы, которых нет в upstream)

| Файл | Назначение |
|---|---|
| `src/server.cpp/.hpp` | headless-сервер: запуск без GUI, режим `--server` |
| `src/ws_client.cpp/.hpp` | WS-клиент к Horizon (лобби, тикеты, админ-события) |

## Наши изменения в общих файлах (суть, не исчерпывающе)

- Транспорт: отказ от P2P-меша — клиент релеит через сервер
  (`transport_udp_default.cpp`, `transport.hpp` и пр.).
- RNG-чекпоинты: команда `DBG rng cp <site> draws=<n> hash=<016X>` для сверки
  детерминизма (randomizer, task*, и др.).
- Логи/дебаг: netlog, ailog, crash_context/reporter — расширения для серверной
  эксплуатации.

> Для точного списка правок по файлу — `git diff --no-index --ignore-cr-at-eol
> max-cpp-port/src src -- <файл>`.

## Rust-ремейк

- **`max-brh/`** — workspace Rust-клиента (бывший client-rust-port): core/net/
  assets/render/app. Референс — `max-cpp-port/` + спецификации `docs/` +
  исследования `max-original-dasm/` + документация автора `max-port-site/`.
- `src/` остаётся сборкой headless-сервера (игровой авторитет для клиента);
  C++-клиент и P2P-режимы больше не развиваются.
