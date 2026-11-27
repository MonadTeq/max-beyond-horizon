//! Manual gate of the GPU anti-aliasing pass: a synthetic frame through the
//! SDL3 GPU FXAA path (`max-brh-docs/development/gpu-aa-design.md`).
//!
//! `nextest` runs headless and without a device, so the shader pass itself cannot
//! be part of the automated suite: this example is the **synthetic test** of the
//! gate. It draws hard-edged geometry at the render resolution of `S = 2`
//! (1-pixel stripes, a 45° line, a circle, flat colour blocks), presents it
//! through [`max_render::SdlWindow`] and toggles the mode with `F`:
//!
//! * `off` — the frame is uploaded by the Canvas path, every edge is a hard
//!   black/white step and every flat block keeps its exact colour;
//! * `fxaa` — the same frame goes through the GPU pass: the edges get
//!   intermediate grey levels, the flat blocks and the 1-pixel stripe pattern of
//!   the left column stay as they are (the flat-area early-out of the shader).
//!
//! Exit codes: `0` when every requested frame was presented (the whole GPU path
//! ran), `1` when the device, the shaders or the swapchain could not be used.
//!
//! ```sh
//! cargo run -p max_render --features sdl-from-source --example gpu_aa -- --frames 90
//! # the same frame through the historical Canvas path, for the visual diff:
//! cargo run -p max_render --features sdl-from-source --example gpu_aa -- --aa off
//! # interactive: omit --frames, `F` toggles FXAA, Esc closes the window
//! ```

use max_render::buffer::RgbaBuffer;
use max_render::render_scale::{AaMode, RenderScale};
use max_render::sdl_window::{SdlWindow, WindowInput, WindowKey};
use max_render::Rgba;

/// Logical frame size of the client (`S = 2` → a 1280×960 render frame).
const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 480;
/// Presentation scale of the probe.
const SCALE: RenderScale = RenderScale::Hd2;

fn main() {
    let mut frames = 0u32;
    let mut aa = AaMode::Fxaa;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                frames = args
                    .next()
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0);
            }
            "--aa" => {
                let value = args.next().unwrap_or_else(|| "fxaa".to_string());

                aa = match AaMode::parse_flag(&value) {
                    Ok(aa) => aa,
                    Err(error) => {
                        eprintln!("gpu_aa: {error}");

                        std::process::exit(2);
                    }
                };
            }
            other => {
                eprintln!("gpu_aa: неизвестная опция {other}");

                std::process::exit(2);
            }
        }
    }

    if let Err(error) = run(frames, aa) {
        eprintln!("gpu_aa: {error}");

        std::process::exit(1);
    }
}

fn run(frames: u32, start: AaMode) -> Result<(), Box<dyn std::error::Error>> {
    let mut window = SdlWindow::new_with_render_scale(
        "M.A.X. GPU AA probe (F — FXAA on/off)",
        FRAME_WIDTH,
        FRAME_HEIGHT,
        SCALE,
        Some(1),
    )
    .map_err(|error| error.to_string())?;

    let mut aa = start;

    window.set_aa(aa);

    let frame = synthetic_frame(FRAME_WIDTH * SCALE.scale(), FRAME_HEIGHT * SCALE.scale());
    let mut presented = 0u32;

    println!(
        "gpu_aa: кадр {}x{}, сглаживание {} (F — переключить, Esc — выход)",
        frame.width(),
        frame.height(),
        window.aa().label()
    );

    loop {
        let (running, events) = window.poll_input();

        if !running {
            break;
        }

        for event in events {
            match event {
                WindowInput::Key(WindowKey::Escape) => return Ok(()),
                WindowInput::Key(WindowKey::Char('f')) | WindowInput::Key(WindowKey::Char('F')) => {
                    aa = if aa.is_off() {
                        AaMode::Fxaa
                    } else {
                        AaMode::Off
                    };
                    window.set_aa(aa);
                    println!("gpu_aa: сглаживание {}", window.aa().label());
                }
                _ => {}
            }
        }

        window.present(&frame).map_err(|error| error.to_string())?;
        presented += 1;

        if frames > 0 && presented >= frames {
            break;
        }
    }

    println!(
        "gpu_aa: OK, presented {presented} frames with {} (effective {})",
        aa.label(),
        window.aa().label()
    );

    Ok(())
}

/// Hard-edged geometry: flat areas, a 1-pixel stripe block, a 45° line, a
/// circle and an axis-aligned bar.
fn synthetic_frame(width: u32, height: u32) -> RgbaBuffer {
    let mut frame = RgbaBuffer::filled(width, height, Rgba::rgb(24, 24, 32));
    let white = Rgba::rgb(240, 240, 240);
    let accent = Rgba::rgb(200, 40, 40);
    let scale = SCALE.scale();

    // Flat blocks: anti-aliasing must not touch their interior.
    frame.fill_rect(
        max_render::PixelRect::new(0, 0, (48 * scale) as i32, (48 * scale) as i32),
        white,
    );
    frame.fill_rect(
        max_render::PixelRect::new(
            (width - 48 * scale) as i32,
            0,
            (48 * scale) as i32,
            (48 * scale) as i32,
        ),
        accent,
    );

    // 1-pixel vertical stripes at render resolution: the hardest case for the
    // early-out — it is *not* flat, so FXAA must reduce the contrast.
    for x in (0..(160 * scale)).step_by(2) {
        for y in (64 * scale)..(160 * scale) {
            frame.set(x as i32, y as i32, white);
        }
    }

    // A 45° line: the staircase FXAA is meant to smooth.
    for step in 0..(200 * scale) {
        let x = 200 * scale + step;

        for thickness in 0..scale {
            frame.set((x + thickness) as i32, (64 * scale + step) as i32, white);
        }
    }

    // A circle: curved edges are where a 1-pixel staircase is visible.
    let (cx, cy) = (320 * scale as i32, 240 * scale as i32);
    let radius = 90 * scale as i32;

    for y in (cy - radius)..(cy + radius) {
        for x in (cx - radius)..(cx + radius) {
            let dx = x - cx;
            let dy = y - cy;

            if dx * dx + dy * dy <= radius * radius {
                frame.set(x, y, white);
            }
        }
    }

    // An axis-aligned bar: a long straight edge with a single-pixel step.
    frame.fill_rect(
        max_render::PixelRect::new(
            (40 * scale) as i32,
            (400 * scale) as i32,
            (240 * scale) as i32,
            (24 * scale) as i32,
        ),
        accent,
    );

    frame
}
