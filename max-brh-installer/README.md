# max-brh-installer — Windows-установщик (NSIS)

Портативный установщик **M.A.X.: Beyond Rust Horizon 0.1.28a** для Windows.
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
`scripts/build-installer.ps1` генерирует `data-groups.nsh` — список `File` по
~8 МБ на группу; после каждой группы `BrhProgress` увеличивает ширину заливки
и показывает процент. Нативная полоса NSIS скрыта. Заголовок MUI и брендинг
скрыты; фон нижней полосы с кнопками окрашивается через
`SetCtlColors $HWNDPARENT`.

Страницы: выбор языка (RU/EN, диалог MUI) → дисклеймер → каталог →
компоненты/ярлык → установка → готово. Деинсталлятор: приветствие →
опции удаления сейвов → удаление → готово.

## Поведение

* Тихая установка: `MAX-BRH-0.1.28a-setup.exe /S /D=<каталог>`.
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
