# max-brh-cicd/scripts — релизные и проверочные скрипты

Windows-сборки — **нативные MSVC-сборки** PowerShell-скриптами на Windows-хосте
(без Docker и mingw): `build-windows-console.ps1` (консольная сборка клиента) и
`build-windows-ui.ps1` (игровая сборка: окно SDL3 + звук). Linux-часть (`live-smoke*.sh`,
`release.sh`, `ci.sh`) по-прежнему идёт через Docker (`rust:1.94.0-bookworm`);
на Windows их запускать из **Git Bash** (не из `cmd`/PowerShell):
`cygpath` и `MSYS_NO_PATHCONV` нужны для корректной передачи путей в Docker.

| Скрипт | Что делает | Требования |
|---|---|---|
| `build-windows-console.ps1` | **нативная MSVC-сборка** `max-brh` — консольная сборка клиента (`--no-default-features`, без SDL3/звука) + упаковка `bin/release/max_app-<ver>-windows-x86_64.zip` + прогон `--help`/`--demo` нативно | Windows, VS с workload «Desktop development with C++» (MSVC + Windows SDK), Rust 1.94.0 (`x86_64-pc-windows-msvc`), данные игры для `--demo` |
| `build-windows-ui.ps1` | **нативная MSVC-сборка** `max-brh` — игровая сборка (окно SDL3 + звук) + упаковка `bin/release/max_app-<ver>-windows-x86_64-ui.zip` | Windows, VS с workload «Desktop development with C++» (MSVC, Windows SDK, CMake), Rust 1.94.0, vcpkg с `sdl3:x64-windows` |
| `ui-test-prefs.ps1` | живой прогон диалога Preferences/Setup: `Setup` в меню (`SETUPPIC`), `Cancel`, кнопка `Preferences` панели (`PREFSPIC`), `Animate Effects`/`FX Disabled` → `effects=0`/`sound=0` в трассе `MAX_DEBUG_INPUT`, `Done`; кадры в `build/ui-test-prefs/` | Windows с **интерактивным рабочим столом** (скриншот через `CopyFromScreen`) и собранная игровая сборка |
| `pe-subsystem.ps1` | читает `Subsystem` из PE-заголовка (2=GUI, 3=Console) без `dumpbin`; используется сборочными скриптами как проверка | Windows PowerShell 5.1 |
| `live-smoke.sh` | живой smoke: Horizon + C++-headless + 2 Rust-клиента × N ходов | собранные `max-brh-server/target/release/horizon` и `build/linux-server-template/max-port` |
| `live-smoke-3.sh` | P5: живой smoke **3 клиента × N ходов** (RED/GREEN/BLUE): релей всем, 00/52/54/45, `state_checksum` у всех, 0 таймаутов | то же; Docker; `--min-players 3` создаётся автоматически |
| `live-smoke-mixed.sh` | P5: **Rust×2 + C++-сервер как desync-оракул**; при отсутствии C++ headless client mode печатает `PARTIAL PASS`/блокер | то же; `MIXED_REQUIRE_CPP=1` — падать вместо partial |

## P5 smoke: 3 клиента и Rust+C++

```sh
bash max-brh-cicd/scripts/live-smoke-3.sh 5
bash max-brh-cicd/scripts/live-smoke-mixed.sh 5
MIXED_REQUIRE_CPP=1 bash max-brh-cicd/scripts/live-smoke-mixed.sh 5   # ожидаемо BLOCKED
```

`live-smoke-3.sh` сам собирает `max-brh` и `horizon_probe`
(`--no-default-features`) и запускает клиентов из `target/debug` — три
параллельных `cargo run` не конкурируют за lock. Порты наружу не публикуются:
probe и клиенты живут в network namespace контейнера Horizon, поэтому занятые
хостом 8080/8082 не мешают. `MAX_LOG=max_app=debug` включает debug-строки
прохождения 52/54/45 и релея; `tracing-subscriber` здесь без `env-filter`,
поэтому одного `RUST_LOG` недостаточно.

`live-smoke-mixed.sh` честно разделяет доказанное и недоказанное:

- **доказано:** два Rust-клиента в одной сессии, CRC совпадает, C++-сервер
  (эталонный `GameManager_CheckDesync`) не пишет `Desync check failed` —
  состояние Rust бит-в-бит совпало с C++-симуляцией;
- **блокер:** headless `src/` собирается только как `--server`
  (`src/main.cpp`: `MAX_HEADLESS` → `Server_Main`), ветки `--client`/`--connect`
  нет; GUI-клиент требует X11 и ручного прохода меню. Полная mixed-сессия
  требует изменения `src/` (headless client mode) либо Xvfb+автоматизации —
  см. `max-brh-docs/architecture/client/protocol.md` §9/§12.

Логи и коды выхода: `build/smoke-logs/live3-*.log`, `live3-ai_log.txt`,
`mixed-*.log`, `mixed-ai_log.txt`. `ai_log.txt` копируется точечно (не весь
`session_root` — там лежит 58-МБ `max-port`).

## build-windows-console.ps1

Нативная MSVC-сборка **headless**-варианта (без окна SDL3, без звука).
Прежняя кросс-сборка `build-windows.sh` (Docker + mingw-w64, target
`x86_64-pc-windows-gnu`) удалена: нативная MSVC-сборка даёт обычный
MSVC-рантайм вместо mingw, без Docker и wine.

```powershell
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1
# только собрать и проверить, без упаковки:
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1 -NoPackage
# не запускать exe (--help/--demo) — на хосте без данных игры:
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1 -SkipSmoke
```

Что делает: находит VS через `vswhere`
(`-requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64`), пишет
`build\msvc-console-build.bat` (там `call vcvars64.bat`, `PATH` +
`~\.cargo\bin`, `CARGO_TARGET_DIR=<repo>\bin\target-console`) и собирает:

```sh
cargo build --release --locked -p max_app --no-default-features
```

target — дефолтный для Windows-rustup `x86_64-pc-windows-msvc` (нативный, без
`--target`). `--no-default-features` = headless: default-фича `max_app`
(`sdl-from-source` → SDL3 3.4.20 через CMake) выключена, поэтому окно SDL3 и
`sound` (rodio/cpal → WASAPI) не включаются, а **vcpkg/SDL3 не нужны вовсе**.
Vendored ENet 1.3.18 (`max_net/build.rs`) компилируется `cc` через `cl.exe`
из vcvars.

Артефакты (`-Out` = `bin\release` по умолчанию):

```
bin/release/max_app-<ver>-windows-x86_64/            max-brh.exe, README.txt,
                                                 LICENSE, CHANGELOG.md, SHA256SUMS
bin/release/max_app-<ver>-windows-x86_64.zip         архив каталога
bin/release/max_app-<ver>-windows-x86_64.zip.sha256  sha256 архива
```

Проверки внутри скрипта (всё — **нативно** на хосте, без wine):

- PE-заголовок читается inline: `MZ`, сигнатура `PE\0\0`, машина x64, `PE32+`;
- подсистема PE — **Console (Subsystem=3)**: подсистема `windows` включается
  только feature `sdl`, а headless собирается без неё, поэтому `--help` и
  `--demo` печатают в терминал/CI (несовпадение — ошибка сборки);
- импорты (`dumpbin /dependents`, лог `build\msvc-console\imports.txt`):
  системные DLL + `VCRUNTIME140.dll`; mingw-рантайма
  (`libgcc_s_*`/`libwinpthread-1.dll`) быть не должно;
- `max-brh.exe --help` печатает справку (ищется «ИСПОЛЬЗОВАНИЕ», rc=0);
- `max-brh.exe --demo max-original\SAVE1.CAM --turns 1` даёт `ticks=1` и
  `state_digest=0x…`; лог прогона —
  `build\smoke-logs\windows-msvc-console-smoke.log`.

Вывод exe снимается через `cmd` с перенаправлением в файл и читается как
UTF-8: так проверка справки не зависит от кодировки консоли PowerShell.

### Особенности сборки, которые остались в коде

- `max_net/build.rs` выбирает платформенный файл ENet по
  `CARGO_CFG_TARGET_OS`, а не по `cfg!(target_os = "windows")`: в build-скрипте
  `cfg!` раскрывается для **хоста**, поэтому при любой кросс-сборке в Windows
  компилировался `unix.c` (он весь под `#ifndef _WIN32` → пустой объект) и
  линковка падала на `undefined reference to enet_socket_create`. При нативной
  MSVC-сборке это неважно, но правило осталось — target-совместимость.
- `tonic` (0.14) подключён с `default-features = false` и явными
  `router,transport,codegen`: смена дефолтов в будущих версиях не притащит
  ring/aws-lc (C+asm) в лёгкую headless-сборку. Для `https://` есть opt-in
  фича `tls` (`max_net/tls` → `max_app/tls`, `tonic/tls-ring` +
  `tonic/tls-webpki-roots`); её зависимости лежат в `Cargo.lock` как
  опциональные и не собираются без фичи.

## build-windows-ui.ps1

Нативная MSVC-сборка **UI**-варианта (окно SDL3 + звук WASAPI).

```powershell
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1 -NoPackage   # только собрать
```

Что делает: находит VS через `vswhere` (`-requires
Microsoft.VisualStudio.Component.VC.Tools.x86.x64`), ищет vcpkg с
`sdl3:x64-windows` (аргумент `-VcpkgRoot`, `$env:VCPKG_ROOT`, затем
`D:\dev\toolchain\vcpkg`, `C:\vcpkg`), генерирует `build/win-msvc-build.bat`
(окружение `vcvars64` + `CARGO_TARGET_DIR`), собирает

```sh
cargo build --release --locked -p max_app --no-default-features --features sdl-vcpkg,sound
```

`--no-default-features` обязателен: `max_app` по умолчанию собирается с
`sdl-from-source` (SDL3 через CMake), и без этого флага к `sdl-vcpkg`
добавилась бы ещё и сборка из исходников.

кладёт `SDL3.dll` рядом с exe, проверяет PE-заголовок (`pe-subsystem.ps1`,
Subsystem=2 = GUI) и `max-brh.exe --help`, затем пакует
`bin/release/max_app-<ver>-windows-x86_64-ui/` (exe + SDL3.dll + README.txt +
LICENSE + CHANGELOG.md + SHA256SUMS) и `.zip` + `.zip.sha256`.

Справка проверяется через `cmd` с перенаправлением в
`bin/target/max-brh-help.txt`: у UI-сборки нет консоли, поэтому
`& max-brh.exe --help | Out-String` в PowerShell не дожидается процесса и
возвращает пустоту, а `cmd` ждёт exit code, и перенаправленный stdout читается
из файла. Тот же приём работает и для ручного запуска:
`max-brh.exe --help > help.txt`.

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
  `CARGO_TARGET_DIR=…\bin\target ` → `failed to create directory … target \release`.
  Поэтому в батнике только `set "VAR=value"`.
- **`SDL3.lib` не находится**: `sdl3-sys` с фичей `use-vcpkg` линкует `SDL3.lib`,
  но путь поиска в `LIB` не добавляет → `LNK1181: cannot open input file
  'SDL3.lib'`. Скрипт дописывает `<vcpkg>\installed\x64-windows\lib` в `LIB`.
- **`SDL3.dll`** должна лежать рядом с exe (без неё exe не стартует) — копируется
  и в `bin/target/release/`, и в архив.
- **PowerShell 5.1 читает .ps1 как ANSI**, если нет BOM: файл сохранён в UTF-8
  **с BOM**, иначе русские строки ломают парсер.
- Сборка идёт в `bin/target/`, а не в `max-brh/target/`, чтобы не
  смешивать MSVC-артефакты с Linux/Docker-сборками.

## pe-subsystem.ps1

Проверка подсистемы PE без `dumpbin`/`vcvars` (PowerShell 5.1):

```powershell
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\pe-subsystem.ps1 `
    -Path bin\release\max_app-1.0.0-windows-x86_64-ui\max-brh.exe -Expected 2
# PE: ...\max-brh.exe — PE32+ x64, Subsystem=2 (Windows GUI)
```

`Subsystem` лежит в Optional Header по смещению `0x44` и для PE32, и для
PE32+ (в PE32+ нет `BaseOfData`, но `ImageBase` шире — смещение не меняется);
абсолютное смещение — `e_lfanew + 4 + 20 + 0x44 = e_lfanew + 0x5C`. Скрипт
проверяет `MZ` и сигнатуру `PE\0\0`, печатает машину и `PE32(+)`, а с
`-Expected 2|3` возвращает exit 1 при несовпадении. Его вызывают
`build-windows-ui.ps1` и `max-brh-qa-tools/max-resources-viewer/build.ps1` (ожидается 2 —
GUI без консоли).

## live-smoke.sh

См. заголовок скрипта; подробности прогона — `max-brh-docs/development/release-checklist.md` §3.
`--handshake-patches` скрипт не передаёт: клиент сам переходит на хэш только
`MAX.RES` после `TP 03 GameData`, а скрипт проверяет `data_hash=<hex>(64)` в
логах. На Git Bash запускать с `MSYS_NO_PATHCONV=1` (иначе `-w /src/...`
превращается в `C:/Program Files/Git/src/...`).
