# Данные шрифтов GNW

Три файла — растровые шрифты оригинальной M.A.X., скопированные из
`max-original/` без изменений:

| Файл здесь | Оригинал | GNW-константа | Высота |
|---|---|---|---|
| `gnw_font_1.fnt` | `FONT1.FON` | `GNW_TEXT_FONT_1` | 16 px |
| `gnw_font_2.fnt` | `FONT2.FON` | `GNW_TEXT_FONT_2` | 9 px (только заглавные) |
| `gnw_font_5.fnt` | `FONT5.FON` | `GNW_TEXT_FONT_5` | 10 px |

Формат разобран в `max-brh/tools/font_vectors/README.md`; декодер —
`max_render::text::gnw::GnwFont::decode`. Тест
`embedded_fonts_match_the_original_files` сверяет вкомпилированные копии с
файлами игры, когда они есть рядом, а `rendering_matches_the_cpp_reference_bitmaps`
— с эталоном C++/FreeType.

Файлы — данные игры (M.A.X. © 1996 Interplay Productions), они не покрыты
MIT-лицензией клиента; в репозитории лежат только для 1:1-совместимости
отрисовки текста, как и остальные извлечённые игровые таблицы.
