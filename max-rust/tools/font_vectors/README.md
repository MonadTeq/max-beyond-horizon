# `font_vectors` — эталонные рендеры шрифтов GNW

Инструмент сверяет два пути отрисовки текста M.A.X.:

1. **C++-порт** (`max-cpp-port/src/text.cpp`): FreeType грузит TTF-ресурсы
   `FONT_1`/`FONT_2`/`FONT_5` из `PATCHES.RES` (`Text_LoadFontTTF`), рисует
   `FT_LOAD_DEFAULT | FT_LOAD_NO_BITMAP` + `FT_RENDER_MODE_MONO` и раскладывает
   глифы как `Text_BlitTTF` (`ascender`, `ulx`, `uly`, `advance`).
2. **Оригинальная игра**: растровые `.FON` (`FONT1.FON`/`FONT2.FON`/
   `FONT5.FON` рядом с `MAX.RES`) — те самые битмапы, которые рисовала GNW.

Для ASCII оба пути дают **побайтово одинаковые** битмапы строк `M.A.X.`,
`Hits 15/15`, `End Turn`, `SURVEY STATUS COLORS`, `044-034` для всех трёх
шрифтов. Поэтому `max_render::text::gnw` использует `.FON` (оригинальный
источник), а `render.txt` служит эталоном для теста
`text::gnw::tests::rendering_matches_the_cpp_reference_bitmaps`.

## Запуск

```sh
docker run --rm -v "D:/dev/max-port-debug:/src" \
    -w /src/max-rust/tools/font_vectors rust:1.94.0-bookworm bash run.sh
```

Результаты пишутся в `build/font-vectors/` (не коммитятся):

| Файл | Что это |
|---|---|
| `vectors.txt` | метрики по каждому ASCII-символу: `adv` и `fon_w` из TTF/`.FON`, `ft_w/ft_h/ulx/uly` из FreeType, флаг совпадения битмапа |
| `render.txt` | ASCII-арт эталонных рендеров (`#` = чернила) — его читает unit-тест |
| `fontN-M-ttf.bmp`, `fontN-M-fon.bmp` | BMP тех же строк; для всех сэмплов файлы совпадают побайтово |

`vectors.txt` и `render.txt` лежат в этой папке как закоммиченные векторы;
BMP и собранный бинарник — только в `build/`.

## Метрики

| Шрифт | Высота (`Text_GetHeight`) | `ascender` FreeType | Совпадение `advance = width` |
|---|---|---|---|
| `GNW_TEXT_FONT_1` (`FONT1.FON`) | 16 | 13 | 91 из 95; `,`/`@`/`Z`/`}` расходятся на 1 px |
| `GNW_TEXT_FONT_2` (`FONT2.FON`) | 9 | 7 | все 95 (шрифт caps-only) |
| `GNW_TEXT_FONT_5` (`FONT5.FON`) | 10 | 8 | все 95 |

Строки в `Text_TextLine`/`Text_TextBox` идут через `height + 1`; тень и
обводка — вправо-вниз от каждого «чернильного» пикселя.

## Формат `.FON` (GNW)

```text
+0x000  u32 version (0x00000100)
+0x004  u32 height (16 / 9 / 10)
+0x008  u32 0
+0x00C  u8[16] подпись/резерв
+0x01C  255 × { u32 width; u32 offset }   глифы символов 1…255
+0x81C  битмапы: строки по ceil(width/8) байт, MSB-first, 1 = чернила
```

Символ 0 — пустой (ширина 0, занимает первые `height` байт зоны битмапов),
поэтому первый `offset` равен `height`. Декодер —
`max_render::text::gnw::GnwFont::decode`; файлы шрифтов лежат в
`max-rust/crates/max_render/data/`.
