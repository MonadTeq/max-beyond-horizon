# max-brh-vendor — вендоринг third-party библиотек

Каталог git-сабмодулей и локальных форков для third-party библиотек
инфраструктуры (вне cargo/crates.io), чтобы сборка была воспроизводимой и не
зависела от сети в момент сборки.

## SDL3

| Аспект | Значение |
|---|---|
| Сабмодуль | `SDL/` → `https://github.com/MonadTeq/mtq-sdl.git` (форк SDL) |
| Версия | **SDL 3.5.0** (commit `c8da179`, ветка `main`; патч — «Fixed error: variable 'vkCreateSamplerYcbcrConversionKHR' set but not used») |
| Форк crate | `sdl3-src-fork/` (имя `sdl3-src`, версия `=3.4.20` для совместимости с `sdl3-sys`; `SOURCE_DIR` указывает на сабмодуль) |
| Механизм | `[patch.crates-io] sdl3-src = { path = "../max-brh-vendor/sdl3-src-fork" }` в `max-brh/Cargo.toml`; форк выставляет `SOURCE_DIR` = `../SDL` |

Используется Rust-клиентом (`max-brh`) через `sdl3` → `sdl3-sys` → `sdl3-src`
(фича `sdl-from-source`, default). Форк меняет единственное: константу
`SOURCE_DIR` — вместо «исходник SDL внутри распакованного crate» она указывает
на сабмодуль `max-brh-vendor/SDL`. Сами исходники SDL в форк **не** копируются.

## Расхождение версий (осознанное)

- **Rust-клиент:** SDL **3.5.0** (форк `MonadTeq/mtq-sdl`, сабмодуль).
- **C++-оракул (`max-cpp-port`):** SDL **3.4.18** (тарбол + md5 в
  `max-cpp-port/dependencies/`).

Расхождение осознанное: каждая сторона проходит свои CRC/поведенческие гейты
независимо. Если в будущем появится баг, зависящий от версии SDL, — смотреть
сюда. Синхронизация версий — отдельная задача при необходимости.

## Порядок обновления SDL

1. `git -C max-brh-vendor/SDL fetch` и `git checkout <новый-коммит>`.
2. Обновить константы `REVISION*` в `sdl3-src-fork/src/lib.rs`.
3. Версию `sdl3-src-fork/Cargo.toml` держать равной constraint'у `sdl3-sys`
   (сейчас `=3.4.20`) — это crate-версия для `[patch.crates-io]`, а не версия SDL.
4. Проверить `cargo tree -p sdl3-src` (должен указывать на форк) и сборку
   `cargo build` с `sdl-from-source`.
