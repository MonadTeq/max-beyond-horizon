# max-brh-installer — установщики и дистрибутивы

Портативные поставки **M.A.X.: Beyond Rust Horizon 0.1.28a**: Windows-установщик
(NSIS, этап 3) и Linux-дистрибутивы (tar.gz + AppImage, этап 4).
Требования: `max-brh-docs/development/distro-installers.md` §D.

## Что устанавливается

Всё — в выбранный каталог; реестр ARP и системные каталоги не используются:

```
<install-dir>/
  game.exe, SDL3.dll
  data/                 509 файлов из max-original + max-cpp-port/assets
                        (MAX.RES, PATCHES.RES, COLOR.PAL, карты, звуки,
                         портреты, музыка, видео, maxint.json)
  remaster/             обязательный HD-remaster (*.ktx2 + manifest)
  saves/, missions/     создаются пустыми; игра пишет сюда
  settings.jsonc        шаблон (существующий файл при обновлении не трогается)
  LICENSE, THIRD_PARTY_LICENSES.txt, README.txt, CHANGELOG.md
  uninstall.exe
```

## Сборка

```powershell
# всё: UI-BMP -> staging (проверка sha256) -> makensis
powershell -ExecutionPolicy Bypass -File max-brh-installer\scripts\build-installer.ps1

# повторная сборка без пересборки staging
powershell -ExecutionPolicy Bypass -File max-brh-installer\scripts\build-installer.ps1 -SkipStage
```

Результат: `max-brh-installer/dist/MAX-BRH-0.1.28a-setup.exe` (+ `build-info.json`).

Промежуточные артефакты — вне git: `.tmp/max-brh-installer/{stage,ui}`.
Требуется NSIS 3.x (`C:\Program Files (x86)\NSIS\makensis.exe`), PowerShell 5.1+.

## Скин

Кастомный UI собран на MUI2 + `nsDialogs` и полностью закрашивает клиентскую
область окна:

| Графика | Использование |
|---|---|
| `resources/installer_ui_main_frame.png` | фон всех страниц (затемнённый кадр с логотипом MAX) |
| `resources/install_in_progress_ui.png` | панель страницы установки; в её слот встроена полоса прогресса |
| `resources/installer_ui_progress_bar.png` | живая заливка полосы: `bar-fill.bmp` обрезается по проценту в `BrhProgress` |
| `resources/installer_ui_banner.png` | кинематографическая полоса на странице «Готово» |
| `resources/game-icon.ico` | иконка установщика/деинсталлятора/ярлыка |

`scripts/prepare-ui.ps1` конвертирует PNG в 24-битные BMP (NSIS не грузит PNG в
static-контролы), генерирует тёмную карточку контента и `bar-fill.bmp`.
`scripts/build-installer.ps1` генерирует `data-groups.nsh` — весь список `File`
(базовые файлы, `data`, `remaster`); после каждых ~8 МБ скопированных байт
`BrhProgress` увеличивает ширину заливки и показывает процент, равный доле уже
скопированных байтов полезной нагрузки. Последний шаг генератора — 99%: 100%
выставляется только после фактического завершения установки (uninstaller,
ярлык), затем страница установки сама уходит на экран «Готово». Кнопка Next на
странице прогресса скрыта — страница не требует клика. Нативная полоса NSIS
скрыта. Заголовок MUI и брендинг скрыты; фон нижней полосы с кнопками
окрашивается через `SetCtlColors $HWNDPARENT`. Текст чекбоксов остаётся
читаемым на тёмной карточке благодаря `UXTHEME::SetWindowTheme` (обход бага
NSIS #443, как в MUI2). На экране «Готово» ряд «Запустить игру» ставится от
измеренной высоты текста, поэтому RU и EN выровнены при любой длине пути.

Страницы: выбор языка (RU/EN, диалог MUI) → дисклеймер → каталог →
компоненты/ярлык → установка → готово. Деинсталлятор: приветствие →
опции удаления сейвов → удаление → готово.

## Поведение

* Тихая установка: `MAX-BRH-0.1.28a-setup.exe /S /D=<каталог>`.
* Авто-переход: экран прогресса не ждёт нажатия — как только все файлы
  скопированы, uninstaller записан и ярлык (если выбран) создан, полоса доходит
  до 100% и мастер сам открывает экран «Готово».
* Ярлык в меню «Пуск» — только при включённом чекбоксе или флаге `/SHORTCUT`;
  тихая установка без флага ярлык не создаёт.
* Обновление «поверх»: файлы игры/данных/remaster перезаписываются,
  `saves/`, `missions/`, `settings.jsonc`, `preferences.ini` не трогаются.
* Удаление: сейвы остаются по умолчанию; чекбокс «Удалить также сейвы»
  (или `/SAVES` в тихом режиме) удаляет `saves/`. Файлы настроек удаляются
  вместе с установкой; сам каталог удаляется, если в нём не осталось сейвов.
* Никаких записей в реестр, кроме опционального ярлыка в пользовательском
  меню «Пуск».

## Тесты G3

```powershell
powershell -ExecutionPolicy Bypass -File max-brh-installer\scripts\test-g3.ps1
```

Проверяются: тихая установка в выбранный каталог, 509 файлов данных по
инвентарю, remaster, запуск `game.exe --new-game` из чужого CWD,
`game.exe --demo` по сейву из `saves/`, ярлык только по `/SHORTCUT`,
апгрейд с сохранением сейвов/настроек, удаление с сохранением сейвов,
`/SAVES`, отсутствие ARP-записей и неизменность оракулов.

## Linux: переносимый tar.gz + AppImage (этап 4)

Обе поставки портативные: системной установки нет, данные, обязательный
remaster и данные игрока живут в одном каталоге.

**Архив** (`max-brh-0.1.28a-linux.tar.xz`, ~247 МиБ) распаковывается в каталог
игры:

```
max-brh-0.1.28a-linux/
  max-brh                    клиент (rpath $ORIGIN/lib)
  lib/libSDL3.so.0           SDL3 в комплекте
  run.sh                     запуск с LD_LIBRARY_PATH (эквивалент)
  data/                      509 файлов, sha256 сверен с data-inventory.json
  remaster/                  обязательный HD-ремастер (*.ktx2 + manifest)
  saves/, missions/          пустые; игра пишет сюда
  settings.jsonc             шаблон окна/разрешения
  LICENSE, THIRD_PARTY_LICENSES.txt, README.txt, CHANGELOG.md
  SHA256SUMS                 хэши всех 526 файлов пакета
```

**AppImage** (`MAX-BRH-0.1.28a-x86_64.AppImage`, ~291 МиБ) — один файл,
двойной клик:

```
AppDir/
  AppRun, max-brh.desktop, max-brh.png (+ .DirIcon)
  data/, remaster/           те же данные и ремастер внутри образа
  usr/bin/max-brh            rpath $ORIGIN/../lib
  usr/lib/libSDL3.so.0
  LICENSE, THIRD_PARTY_LICENSES.txt, README.txt, CHANGELOG.md, settings.jsonc
```

Образ смонтирован только для чтения, поэтому **каталог игры — каталог самого
`.AppImage`**: `layout::detect` учитывает переменную `APPIMAGE` и ставит
`game_root` туда, `AppRun` создаёт рядом `saves/`/`missions/`, кладёт шаблон
`settings.jsonc` (если его ещё нет) и передаёт `--remaster-dir` внутрь образа.
Сейвы, миссии, `preferences.ini` и `settings.jsonc` не покидают каталог игрока.

### Сборка

```powershell
# Docker-образ (Ubuntu 22.04 + Rust 1.94.0 + appimagetool 1.9.1) + упаковка + гейт:
powershell -ExecutionPolicy Bypass -File max-brh-installer\scripts\build-linux-distro.ps1

# переиспользовать собранный Linux-бинарь (.tmp/linux-dist/target)
powershell ... -SkipBuild

# только гейт G4 по готовым артефактам
powershell ... -GateOnly
```

Артефакты — `max-brh-installer/dist/linux/` (вне git): архив, AppImage,
`SHA256SUMS`, `staging-report.json`, `g4-summary.json`, `g4.log`. Промежуточные
файлы — `.tmp/linux-dist/`. В контейнере работают `linux/build-distro.sh`
(сборка + упаковка) и `linux/gate-g4.sh` (гейт), исходники `linux/AppRun` и
`linux/max-brh.desktop`; `linux/Dockerfile` — образ.

### Гейт G4

`build-distro.sh` сверяет каждый файл `data/` с `data-inventory.json` (размер +
sha256), собирает детерминированный архив (фиксированные mtime/owner/порядок) и
сравнивает размер `tar.gz` и `tar.xz`, оставляя меньший (сейчас `.tar.xz`,
на 48 МиB меньше).

`gate-g4.sh` в чистом каталоге контейнера проверяет: хэши артефактов →
распаковку (509 файлов/385 718 310 байт + remaster) → `sha256sum -c SHA256SUMS`
→ `ldd` без `not found` и SDL3 из `lib/` → headless `--new-game`/`--demo` из
чужого CWD (1828 записей MAX.RES, 84 PATCHES.RES, 0 отсутствующих файлов) →
GUI под Xvfb (окно, `preferences.ini` читается из каталога игры) →
`--appimage-extract` → запуск AppImage под Xvfb с `APPIMAGE_EXTRACT_AND_RUN=1`
(в контейнере нет FUSE): окно, `saves/`/`missions/`/`settings.jsonc` рядом с
файлом, `preferences.ini` читается из каталога `.AppImage` → оракулы не тронуты.

Известное косметическое сообщение при запуске: `assets/PATCHES.RES не найден;
PATCHES.RES грузится из fallback data/PATCHES.RES` — штатный fallback
портативной раскладки (файл найден рядом с MAX.RES); `data_hash` не меняется.
