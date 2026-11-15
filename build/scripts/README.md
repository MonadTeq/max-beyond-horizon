# build/scripts — релизные и проверочные скрипты

Скрипты рассчитаны на Git Bash / WSL / Linux. Хостовый Rust-тулчейн не нужен:
всё, что собирается, собирается в Docker (`rust:1.94.0-bookworm`). На Windows
запускать из **Git Bash** (не из `cmd`/PowerShell): `cygpath` и `MSYS_NO_PATHCONV`
нужны для корректной передачи путей в Docker.

| Скрипт | Что делает | Требования |
|---|---|---|
| `build-windows.sh` | кросс-сборка **Windows x86_64** бинаря `max_app` (headless) + упаковка в `release/` + wine-smoke | Docker, сеть (apt + crates.io) |
| `live-smoke.sh` | живой smoke: Horizon + C++-headless + 2 Rust-клиента × N ходов | собранные `max-horizon-server/target/release/horizon` и `build/linux-server-template/max-port` |

## build-windows.sh

```sh
bash build/scripts/build-windows.sh             # сборка + zip + wine-smoke
bash build/scripts/build-windows.sh --no-wine   # без wine (~400 МБ apt)
bash build/scripts/build-windows.sh --dry-run   # план, Docker не запускается
bash build/scripts/build-windows.sh --out /tmp/rel
```

В контейнере `rust:1.94.0-bookworm` скрипт ставит
`gcc-mingw-w64-x86-64` + `binutils-mingw-w64-x86-64` (ими собирается vendored
ENet 1.3.18), `file`, `zip`, опционально `wine`, добавляет target
`x86_64-pc-windows-gnu` и выполняет:

```sh
cargo build --release --locked --target x86_64-pc-windows-gnu \
            -p max_app --no-default-features
```

`--no-default-features` = headless: фичи `sdl*` (SDL3 из исходников/vcpkg —
нужен CMake/MSVC и Windows-импорты) и `sound` (rodio/cpal → WASAPI) не
включаются. Полная SDL-сборка — только на Windows-хосте, инструкция лежит в
`release/max_app-<версия>-windows-x86_64/README.txt` и в
`docs/release-checklist.md` §6.

Артефакты (`$OUT` = `release/` по умолчанию):

```
release/max_app-<ver>-windows-x86_64/            max_app.exe, README.txt,
                                                 LICENSE, CHANGELOG.md, SHA256SUMS
release/max_app-<ver>-windows-x86_64.zip         архив каталога
release/max_app-<ver>-windows-x86_64.zip.sha256  sha256 архива
```

Проверки внутри скрипта:

- PE-заголовок: `file` → `PE32+ executable (console) x86-64`,
  `objdump -f` → `file format pei-x86-64`, первые байты `MZ`;
- импорты перечисляются в логе; `libgcc_s_seh-1.dll`/`libwinpthread-1.dll` в
  них быть не должно (иначе exe не запустится без mingw-рантайма) — это
  проверяется как ошибка сборки;
- wine-smoke (если wine поставлен): `max_app.exe --help` и
  `max_app.exe --demo SAVE1.CAM --turns 1` в каталоге с сейвом
  `max-original/SAVE1.CAM`; лог — `build/smoke-logs/windows-wine-smoke.log`.

Про wine: в Debian bookworm (база образа) wine 8.0, а `std` Rust импортирует
`bcryptprimitives!ProcessPrng` — этой DLL в wine 8.0 нет, и exe падает на
загрузке (exit 53, «Importing dlls failed, status c0000135»). Скрипт не угадывает
версию: сначала запускает exe «как есть», и только если `--help`/`--demo` не
прошли, подкладывает рядом с копией exe **тестовый** шим
`bcryptprimitives.dll` (`ProcessPrng` → `advapi32!SystemFunction036`) и
повторяет прогон. В релизный zip шим **не входит** — на настоящей Windows
`bcryptprimitives.dll` системная. Вердикт виден в логе:
`WINE: ok (…; шим bcryptprimitives: да|нет)` или `WINE: failed (…)`. Если wine
не установился, шаг помечается `WINE: skipped` и сборка не падает; сделать
wine-smoke обязательным можно через `WINE_STRICT=1`.

Переменные: `RUST_IMAGE`, `OUT`, `WINE_STRICT=1`.

### Что пришлось поправить ради кросс-сборки

- `max_net/build.rs` выбирал файл ENet по `cfg!(target_os = "windows")` — это
  cfg **хоста**, на котором компилируется build-скрипт. При кросс-сборке в
  Windows получался `unix.c` (он весь под `#ifndef _WIN32` → пустой объект) и
  линковка падала на `undefined reference to enet_socket_create`. Теперь
  платформа берётся из `CARGO_CFG_TARGET_OS`.
- `tonic` (0.14) подключён с `default-features = false` и явными
  `router,transport,codegen`. У 0.14 дефолт и так без TLS (в `Cargo.lock`
  rustls/ring не было), но набор зафиксирован явно: смена дефолтов в будущих
  версиях не притащит ring/aws-lc (C+asm) в кросс-сборку. Для `https://` есть
  opt-in фича `tls` (`max_net/tls` → `max_app/tls`, `tonic/tls-ring` +
  `tonic/tls-webpki-roots`); её зависимости лежат в `Cargo.lock` как
  опциональные и не собираются без фичи.

## live-smoke.sh

См. заголовок скрипта; подробности прогона — `docs/release-checklist.md` §3.
