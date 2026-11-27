# GPU anti-aliasing (FXAA/SMAA) — дизайн и реализация

Версия 1.0 (дизайн Графического программиста · реализация Разработчика) ·
2026-11-27. Заменяет `ssaa-design.md` (устарел).

## Резюме

| Вопрос | Решение |
|---|---|
| Бэкенд | **SDL3 GPU API (`SDL_gpu`)** — уже в вендоре MonadTeq 3.5.0 + в `sdl3` 0.22.1 (`sdl3::gpu`). Не wgpu/glow. |
| Шейдер-язык | GLSL → **SPIR-V** (прекомпиляция, `include_bytes!`), бэкенд Vulkan на Win/Linux. DXIL/MSL — позже. |
| Пайплайн | CPU-кадр `640·S` → upload → **FXAA-проход** → blit в swapchain по `content_rect`. |
| Шейдер | **FXAA** (1 проход) — реализован; SMAA — фаза 2. |
| Параметр | `AaMode {Off,Fxaa,Smaa}`; `aa` — параметр **слоя вывода**, не рендера. |
| Fallback | нет устройства/шейдера/swapchain → SDL3 Canvas, `aa→Off` (с логом). |

## Ключевые решения

1. **FXAA в разрешении рендер-кадра** (`640·S`), затем blit с фильтром режима —
   не после апскейла.
2. **`aa` — чистый параметр вывода**, не рендера: `render_frame_hd` рисует `S`,
   `SdlWindow::present` выбирает Canvas/FXAA/SMAA.
3. **PixelPerfect (S=1) в GPU-путь не заходит** (байт-в-байт Canvas).
4. **Uniform whole-frame**: сглаживается весь собранный кадр (игра + меню-бар +
   открытый dropdown), а не отдельные слои.
5. CPU-SSAA (`downscale_box`) удалён из презентационного пути и из кода.

## Реализация (как устроено)

### Слои

| Слой | Модуль | Что делает |
|---|---|---|
| чистый | `max_render::gpu_aa` | `blit_regions` (клип `content_rect` к swapchain), `FxaaParams` (uniform-блок), `FXAA_LUMA_THRESHOLD` |
| SDL | `max_render::gpu_present` | `GpuPresenter`: устройство, текстуры, пайплайн, проход, blit, submit |
| SDL | `max_render::sdl_window` | выбор бэкенда: `aa=Off` → Canvas, иначе GPU; fallback |
| приложение | `max_app::main::WindowPresentation` | `set_aa` из CLI/jsonc/меню, кадр всегда `S` |

`GpuPresenter::present(&RgbaBuffer, ScreenGeometry, FilterMode)` за один
command buffer:

```text
wait_and_acquire_swapchain_texture
  copy pass:  upload RgbaBuffer → source texture (R8G8B8A8_UNORM, SAMPLER)
  render pass: fullscreen triangle, FXAA → aa texture (COLOR_TARGET|SAMPLER)
  blit:        aa texture → swapchain, content_rect, CLEAR (letterbox), фильтр S
  submit
```

Шейдеры: `max-brh/crates/max_render/shaders/fxaa.vert`, `fxaa.frag` (GLSL 450,
исходники в репозитории) и `*.spv` (прекомпилированные, вкомпилированы через
`include_bytes!`). Порядок ресурсов — конвенция SDL3 GPU для SPIR-V:
`set 2` — фрагментные сэмплеры, `set 3` — фрагментные uniform-блоки; uniform
заполняется `SDL_PushGPUFragmentUniformData(0, …)`.

Тексельный шаг шейдер берёт из самой текстуры (`1.0 / textureSize(...)`), а не из
uniform — потеря/неинициализированный uniform тогда ухудшает качество (порог
плоскости `0`), но **не превращает проход в копию кадра**. Сам порог плоскости
(`FXAA_LUMA_THRESHOLD = 1/32`) — единственное содержимое uniform-блока.

### Один swapchain на окно (важно)

SDL не разрешает два swapchain на одно нативное окно: рендерер Canvas-пути
занимает окно при создании, и после этого `SDL_ClaimWindowForGPUDevice` падает с
`VK_ERROR_NATIVE_WINDOW_IN_USE_KHR`. Поэтому `SdlWindow` держит **ровно один**
бэкенд:

```rust
enum WindowOutput { Canvas(Canvas<Window>), Gpu { presenter, window }, Idle(Window), Taken }
```

Смена режима освобождает текущий бэкенд (`release_backend`) и создаёт нужный на
следующем кадре; смена разрешения пересоздаёт окно и оставляет его без бэкенда.
`aa=Off` и первый кадр после переключения идут через Canvas.

### SMAA

`AaMode::Smaa` принимается флагом и файлом (чистый разрыв значений), но пока
исполняется тем же FXAA-проходом; в лог пишется, что SMAA — фаза 2. Меню
показывает выбранный режим (`SMAA`), `SdlWindow::aa()` — фактический.

### Почему Vulkan only

Устройство создаётся с `ShaderFormat::SPIRV` (решение владельца), поэтому SDL
выбирает Vulkan-бэкенд; DXIL/MSL потребовали бы второй копии шейдеров и выходят
за рамки этой фазы.

## Спайк (этап 1)

Проверено на вендоре MonadTeq **SDL 3.5.0** (`max-brh-vendor/SDL`, коммит
`c8da179`) с Rust-клиентом `sdl3` 0.22.1 → `sdl3-sys` 0.7.3 (**bindings 3.4.20**):

| Проверка | Результат |
|---|---|
| `SDL_GPU` в сборке вендора | CMakeCache собранного SDL: `SDL_GPU:BOOL=ON`, `SDL_VULKAN:BOOL=ON` |
| Создание устройства «только SPIR-V» | `ShaderFormat(2)` = SPIRV; драйвер Vulkan |
| Claim окна / swapchain | `B8g8r8a8Unorm`, 91 кадр презентован в спайке |
| FXAA-пайплайн из SPIR-V | создан и исполнен (см. гейты) |
| ABI 3.4.20 ↔ 3.5.0 | **расхождений не обнаружено**: используемые функции GPU API стабильны в 3.x, все вызовы прошли |

Расхождение версий (bindings 3.4.20, библиотека 3.5.0) осознанное и то же, что
для остального клиента (`max-brh-vendor/README.md`): SDL держит ABI внутри
мажорной версии. Если появится отказ конкретного вызова GPU API — эскалация,
синхронизация на `sdl3-sys` 3.5.x отдельной задачей.

## Гейты (DoD)

Автоматические (`cargo nextest run -p max_render -p max_app --no-default-features`,
без GPU/окна):

| Гейт | Где |
|---|---|
| PixelPerfect 1:1 байт-в-байт (aa=Off, S=1) | существующие golden-CRC/QA + `render_tests` |
| `aa=Off` (HD) == текущий HD-кадр | `hd_presentation::the_hd_frame_is_the_current_hd_frame` |
| snapshot/CRC не зависят от aa | `presentation_gate::antialiasing_does_not_change_the_snapshot_or_digest` |
| `AaMode` = off/fxaa/smaa, `2`/`4` — ошибка | `render_scale_tests`, `main_tests`, `settings_tests` |
| клип blit-регионов (letterbox/crop/swapchain меньше кадра) | `gpu_aa_tests` |
| uniform-блок = 16 байт std140 | `gpu_aa_tests` |
| `S=1` принудительно Off | `settings_tests::pixel_perfect_forces_antialiasing_off` |

Ручной (нужен GPU-девайс; `nextest` headless его не запускает) —
**синтетический тест FXAA**: `crates/max_render/examples/gpu_aa.rs`.

```sh
cargo run -p max_render --features sdl-from-source --example gpu_aa -- --frames 90
cargo run -p max_render --features sdl-from-source --example gpu_aa -- --aa off
```

Кадр `S=2` с жёсткой геометрией (круг, линия 45°, полосы 1 px, плоские блоки,
полоса-прямоугольник); `F` переключает режим на лету. Проверено (скриншоты
сравнены попиксельно):

- края круга и линии 45° получают промежуточные уровни (FXAA работает);
- плоские участки (интерьер блока, фон) — **байт-в-байт** как при `off`;
- ориентация кадра не инвертируется (viewport SDL3 GPU флипнут: `y=+1` — верх).

Fallback: при недоступном устройстве/шейдере печатается одна строка
`SDL: GPU-сглаживание недоступно (…) — сглаживание переключено на off` и кадр
идёт через Canvas.

## Производительность

Цель из дизайна: `S=2 + FXAA < 1 мс` на GPU. Замер — на живой машине примером
`gpu_aa` (headless-бенчмарк `hd_frame_bench` меряет только CPU-кадр `S` и от
`aa` не зависит). CPU-путь не изменился: кадр `S` и его время те же, что до
GPU-AA; сглаживание ушло с CPU (box-фильтр `S·aa` дороже на порядок).

## Развилки владельца (решение 2026-11-27)

1. **Заменить SSAA** на GPU-AA (`AaMode → Off/Fxaa/Smaa`).
2. **Чистый разрыв** `--aa` (off|fxaa|smaa; 2|4 — ошибка).
3. **Только Vulkan + SPIR-V**; DXIL позже.
4. **FXAA сейчас**, SMAA — фаза 2.

## Перегенерация шейдеров

`.spv` коммитятся; сборке GLSL-компилятор не нужен (`build.rs` следит за
`shaders/*`). Пересборка:

```sh
glslangValidator -V --target-env vulkan1.0 -o fxaa.vert.spv fxaa.vert
glslangValidator -V --target-env vulkan1.0 -o fxaa.frag.spv fxaa.frag
```

SPIR-V 1.0 (проверяется `words[1] == 0x00010000`) — минимум для Vulkan 1.0.
