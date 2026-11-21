# Анализ

Материалы анализа: реверс-инжиниринг оригинала, сверка визуала и поведения с C++-портом.

| Документ | Содержимое |
|---|---|
| [visual-1to1-status.md](visual-1to1-status.md) | Статус визуального паритета 1:1: форматы ресурсов (WRL, simple/big image, спрайты, курсоры), раскладка окна, план A–F, риски |
| [reverse-engineering.md](reverse-engineering.md) | Указатель на RE-артефакты в `max-original-dasm/` |

## Где лежат RE-материалы

Основные артефакты дизассемблирования — в `max-original-dasm/`:
- `TOOLS.md` — инструменты и процедуры;
- `tools/dosbox/entry-strategy.md`, `runtime-placement.md` — динамический анализ (вход 0x26C714 и пр.);
- `functions-named.csv` — каталог функций с именами C++;
- `formats/*.md` — спецификации форматов (RES, WRL, SPW/спрайты);
- эталонные Python-парсеры в `tools/`.
