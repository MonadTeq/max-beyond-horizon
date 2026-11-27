# План SSAA (super-sampling anti-aliasing)

Версия 0.1 (дизайн Графического программиста) · 2026-11-27. Реализацию делает Разработчик.

## Резюме

| Вопрос | Решение |
|---|---|
| Встраивание | `aa` — ортогональная ось поверх `S`, только в презентации (`max_app` + чистая функция `max_render`). `SdlWindow`/`max_core`/snapshot/CRC не трогаются. |
| Геометрия | рендер `640·S·aa` → даунсэмпл → `640·S`; окно `640·S·window_scale` (от `aa` не зависит). |
| Фильтр | **Box 2×2/4×4 на CPU** (целочисленный, детерминированный). SDL Linear отвергнут (недетерминирован + не box). |
| Параметр | `AaMode { Off, X2, X4 }`; CLI `--aa 0|off|1|2|4`; `settings.jsonc display.aa`; GUI `Settings→Antialiasing`; приоритет CLI > jsonc > дефолт Off. |
| S=1 | `aa` принудительно Off (golden 1:1 байт-в-байт). |
| 60 fps | Нет на CPU при S=2+aa; `aa` — опциональный пресет качества, дефолт Off. |

## Порядок работ (Разработчик)

1. `max_render`: `AaMode` (`render_scale.rs`) + `downscale_box` (`frame_scale.rs`) + реэкспорт + unit-тесты.
2. `max_app::settings`: `display.aa`, `CliPresentation.aa`, `PresentationSettings.aa`, `resolve` + S=1→Off.
3. `main.rs` CLI: `--aa` парсинг/валидация.
4. `render` + `ui`: прокинуть `aa` в `render_frame_hd*`/`render_session_hd`; HD-адаптеры — nearest ×aa в S·aa.
5. `WindowPresentation`: поле `aa`, `ss_buffer`, ветка `present` (Off=текущий / aa>1=downscale), `set_aa` без переоткрытия окна.
6. `window_menu.rs`: пункт Antialiasing (Off/2x/4x) + `SetAa`.
7. Гейты: `presentation_gate.rs` (aa-детерминизм), `hd_presentation.rs` (aa=off identity + SSAA-golden), `hd_frame_bench.rs --aa`.
8. Доки: `remaster-hd.md`, шапка `settings.rs`, шаблон `settings.jsonc`.

## Гейты (DoD)

- PixelPerfect 1:1 байт-в-байт (aa=Off при S=1).
- aa=off identity (== текущий HD-кадр).
- aa=2x/4x сглаживает края (интерьеры не меняются).
- Детерминизм: snapshot/CRC не зависят от aa.
- VRAM: SSAA-буфер — CPU RAM, не VRAM (квота HdStats остаётся ×S).

## Развилки владельца (решение 2026-11-27)

1. UI-хром/шрифт: **uniform** (whole-frame resolve).
2. `aa` дефолт: **Off** (опциональный пресет качества/скриншотов).
3. Диапазон `aa`: **только {2,4}** (целые box; 3/нецелые — ошибка запуска).
