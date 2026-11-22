# max-brh-vendor — вендоринг third-party библиотек

Каталог git-сабмодулей и локальных форков для third-party библиотек
инфраструктуры (вне cargo/crates.io), чтобы сборка была воспроизводимой и не
зависела от сети в момент сборки.

## SDL3

| Аспект | Значение |
|---|---|
| Сабмодуль | `SDL/` → `https://github.com/libsdl-org/SDL.git` |
| Версия/тег | **`release-3.4.20`** (commit `f00dc919ca35b9756316e86c4777392de442f9b2`) |
| Форк crate | `sdl3-src-fork/` (имя `sdl3-src`, версия `=3.4.20`) |
| Механизм | `[patch.crates-io] sdl3-src = { path = "../max-brh-vendor/sdl3-src-fork" }` в `max-brh/Cargo.toml`; форк выставляет `SOURCE_DIR` = `../SDL` |

Используется Rust-клиентом (`max-brh`) через `sdl3` → `sdl3-sys` → `sdl3-src`
(фича `sdl-from-source`, default). Форк меняет единственное: константу
`SOURCE_DIR` — вместо «исходник SDL внутри распакованного crate» она указывает
на сабмодуль `max-brh-vendor/SDL`. Сами исходники SDL в форк **не** копируются.

## Расхождение версий (осознанное)

- **Rust-клиент:** SDL **3.4.20** (сабмодуль `max-brh-vendor/SDL`).
- **C++-оракул (`max-cpp-port`):** SDL **3.4.18** (тарбол + md5 в
  `max-cpp-port/dependencies/`).

Расхождение осознанное: каждая сторона проходит свои CRC/поведенческие гейты
независимо. Если в будущем появится баг, зависящий от версии SDL, — смотреть
сюда. Синхронизация версий — отдельная задача при необходимости.

## Порядок обновления SDL

1. `git -C max-brh-vendor/SDL fetch --tags` и `git checkout <новый-тег>`.
2. Обновить константы `REVISION*` в `sdl3-src-fork/src/lib.rs`.
3. Обновить версию `sdl3-src-fork/Cargo.toml` под constraint `sdl3-sys`
   (сейчас `=3.4.20`).
4. Проверить `cargo tree -p sdl3-src` (должен указывать на форк) и сборку
   `cargo build` с `sdl-from-source`.
