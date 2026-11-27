# GPU anti-aliasing (FXAA/SMAA) — план

Версия 0.1 (дизайн Графического программиста) · 2026-11-27. Реализует Разработчик.

## Резюме

| Вопрос | Решение |
|---|---|
| Бэкенд | **SDL3 GPU API (`SDL_gpu`)** — уже в вендоре MonadTeq 3.5.0 + в `sdl3` 0.22.1 (`sdl3::gpu`). Не wgpu/glow. |
| Шейдер-язык | GLSL → **SPIR-V** (прекомпиляция, `include_bytes!`), бэкенд Vulkan на Win/Linux. DXIL/MSL — позже. |
| Пайплайн | CPU-кадр `640·S×496·S` → upload → **FXAA-проход** → blit в swapchain по `content_rect`. |
| Шейдер | **FXAA** (1 проход) старт; SMAA — фаза 2. |
| Параметр | `AaMode {Off,X2,X4}` → **`{Off,Fxaa,Smaa}`**; `aa` переходит из рендера в слой вывода. |
| Fallback | нет устройства/шейдера → SDL3 Canvas, `aa→Off`. |

## Ключевые решения

1. **FXAA в разрешении рендер-кадра** (`640·S`), затем blit с фильтром режима — не после апскейла.
2. **`aa` — чистый параметр вывода**, не рендера: `render_frame_hd` рендерит `S`, `SdlWindow::present` выбирает Canvas/FXAA/SMAA.
3. **PixelPerfect (S=1) в GPU-путь не заходит** (байт-в-байт Canvas).
4. **Uniform whole-frame** (сохранено решение владельца).
5. CPU-SSAA (`downscale_box`) — уходит из презентационного пути.

## Этапы (Разработчик)

1. Спайк: `sdl3` gpu-примеры против вендора 3.5.0, `SDL_GPU=ON`, ABI 3.4.20↔3.5.0.
2. `AaMode {Off,Fxaa,Smaa}` + `parse_flag`.
3. `max_render::gpu_present` (feature sdl): `GpuPresenter` + SPIR-V + FXAA-проход + blit.
4. `SdlWindow::present`: aa=Off → Canvas; иначе GPU; fallback.
5. `max_app`: убрать `ss_buffer`/`render_factor·aa`; `--aa`/`display.aa`/GUI → off|fxaa|smaa.
6. (опц.) SMAA — 2-3 прохода + LUT.
7. Доки/тесты.

## Гейты (DoD)

- PixelPerfect 1:1 байт-в-байт.
- aa=Off (HD) == текущий HD-кадр.
- FXAA сглаживает края, не трогает flat-области (синтетический тест).
- snapshot/CRC не зависят от aa.
- S=2+FXAA < 1 мс на GPU; 60 fps ограничено CPU-рендером S=2 (~47 fps), не AA.
- Fallback: Canvas + aa→Off.

## Развилки владельца (решение 2026-11-27)

1. **Заменить SSAA** на GPU-AA (`AaMode → Off/Fxaa/Smaa`).
2. **Чистый разрыв** `--aa` (off|fxaa|smaa; 2|4 — ошибка).
3. **Только Vulkan + SPIR-V**; DXIL позже.
4. **FXAA сейчас**, SMAA — фаза 2.
