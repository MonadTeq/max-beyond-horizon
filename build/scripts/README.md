# build/scripts — релизные и проверочные скрипты

`build-windows.sh` и `live-smoke.sh` рассчитаны на Git Bash / WSL / Linux и
работают через Docker (`rust:1.94.0-bookworm`) — хостовый Rust-тулчейн им не
нужен. На Windows их запускать из **Git Bash** (не из `cmd`/PowerShell):
`cygpath` и `MSYS_NO_PATHCONV` нужны для корректной передачи путей в Docker.
`build-windows-ui.ps1` — наоборот, PowerShell-скрипт для **Windows-хоста**:
он собирает интерактивный клиент нативным MSVC.

| Скрипт | Что делает | Требования |
|---|---|---|
| `build-windows.sh` | кросс-сборка **Windows x86_64** бинаря `max_app` (headless) + упаковка в `release/` + wine-smoke | Docker, сеть (apt + crates.io) |
| `build-windows-ui.ps1` | **нативная** Windows-сборка `max_app` с окном SDL3 и звуком + упаковка `release/max_app-<ver>-windows-x86_64-ui.zip` | Windows, VS с workload «Desktop development with C++» (MSVC, Windows SDK, CMake), Rust 1.94.0, vcpkg с `sdl3:x64-windows` |
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
`docs/development/release-checklist.md` §6.

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

## build-windows-ui.ps1

Нативная Windows-сборка **интерактивного** клиента (окно SDL3 + звук WASAPI).

```powershell
powershell -ExecutionPolicy Bypass -File build\scripts\build-windows-ui.ps1
powershell -ExecutionPolicy Bypass -File build\scripts\build-windows-ui.ps1 -NoPackage   # только собрать
```

Что делает: находит VS через `vswhere` (`-requires
Microsoft.VisualStudio.Component.VC.Tools.x86.x64`), ищет vcpkg с
`sdl3:x64-windows` (аргумент `-VcpkgRoot`, `$env:VCPKG_ROOT`, затем
`D:\dev\toolchain\vcpkg`, `C:\vcpkg`), генерирует `build/win-msvc-build.bat`
(окружение `vcvars64` + `CARGO_TARGET_DIR`), собирает

```sh
cargo build --release --locked -p max_app --features sdl-vcpkg,sound
```

кладёт `SDL3.dll` рядом с exe, проверяет `max_app.exe --help` и пакует
`release/max_app-<ver>-windows-x86_64-ui/` (exe + SDL3.dll + README.txt +
LICENSE + CHANGELOG.md + SHA256SUMS) и `.zip` + `.zip.sha256`.

Установка тулчейна (Windows, без администратора — кроме самого VS):

```powershell
Invoke-WebRequest https://win.rustup.rs/x86_64 -OutFile $env:TEMP\rustup-init.exe
& $env:TEMP\rustup-init.exe -y --default-toolchain 1.94.0 --profile minimal
git clone --depth 1 https://github.com/microsoft/vcpkg D:\dev\toolchain\vcpkg
D:\dev\toolchain\vcpkg\bootstrap-vcpkg.bat -disableMetrics
D:\dev\toolchain\vcpkg\vcpkg.exe install sdl3:x64-windows
# MSVC + Windows SDK — только установщиком Visual Studio, workload
# "Desktop development with C++" (нужны права администратора)
```

Грабли, которые скрипт обходит (проверено на этом хосте):

- **cmd `set VAR=value && …`** записывает в значение пробел перед `&&`:
  `CARGO_TARGET_DIR=…\win-msvc-target ` → `failed to create directory … target \release`.
  Поэтому в батнике только `set "VAR=value"`.
- **`SDL3.lib` не находится**: `sdl3-sys` с фичей `use-vcpkg` линкует `SDL3.lib`,
  но путь поиска в `LIB` не добавляет → `LNK1181: cannot open input file
  'SDL3.lib'`. Скрипт дописывает `<vcpkg>\installed\x64-windows\lib` в `LIB`.
- **`SDL3.dll`** должна лежать рядом с exe (без неё exe не стартует) — копируется
  и в `build/win-msvc-target/release/`, и в архив.
- **PowerShell 5.1 читает .ps1 как ANSI**, если нет BOM: файл сохранён в UTF-8
  **с BOM**, иначе русские строки ломают парсер.
- Сборка идёт в `build/win-msvc-target/`, а не в `max-rust/target/`, чтобы не
  смешивать MSVC-артефакты с Linux/Docker-сборками.

## live-smoke.sh

См. заголовок скрипта; подробности прогона — `docs/development/release-checklist.md` §3.
`--handshake-patches` скрипт не передаёт: клиент сам переходит на хэш только
`MAX.RES` после `TP 03 GameData`, а скрипт проверяет `data_hash=<hex>(64)` в
логах. На Git Bash запускать с `MSYS_NO_PATHCONV=1` (иначе `-w /src/...`
превращается в `C:/Program Files/Git/src/...`).
