//! GPU anti-aliasing presentation: the SDL3 GPU FXAA pass
//! (`max-brh-docs/development/gpu-aa-design.md`).
//!
//! This is the output layer of the remaster: it takes the finished
//! `640·S × 480·S` RGBA8 render frame, smooths it with one FXAA pass **at that
//! resolution** and blits the result into the swapchain content rectangle with
//! the filter of the presentation mode. Anti-aliasing is therefore applied
//! before the upscale, never after it, and the CPU never allocates a
//! super-sampled buffer.
//!
//! # Frame flow (one command buffer per frame)
//!
//! ```text
//! RgbaBuffer ──upload──▶ source texture ──FXAA pass──▶ AA texture ──blit──▶ swapchain
//!             (copy pass)                  (render pass)            (content rect)
//! ```
//!
//! `aa = Off` never reaches this module: [`crate::SdlWindow`] uploads the frame
//! straight through the SDL renderer (Canvas), which is the byte-for-byte
//! historical path and the fallback when no GPU device or shader is available.
//!
//! # Shaders
//!
//! `shaders/fxaa.vert` and `shaders/fxaa.frag` are GLSL sources kept in the
//! repository; the compiled SPIR-V next to them is what this module embeds
//! (`include_bytes!`), because the build must not require `glslangValidator`.
//! `shaders/README.md` has the exact regeneration command.
//!
//! # Why Vulkan only
//!
//! The device is created with [`ShaderFormat::SPIRV`] only (owner decision of
//! 2026-11-27), so SDL picks its Vulkan backend: DXIL/MSL would need a second
//! shader copy and are out of scope for this phase.

use sdl3::gpu::{
    BlitInfo, ColorTargetDescription, ColorTargetInfo, CullMode, Device, FillMode, Filter,
    GraphicsPipeline, GraphicsPipelineTargetInfo, LoadOp, PrimitiveType, RasterizerState, Sampler,
    SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode, ShaderFormat, ShaderStage, StoreOp,
    Texture, TextureCreateInfo, TextureFormat, TextureRegion, TextureSamplerBinding,
    TextureTransferInfo, TextureType, TextureUsage, TransferBufferUsage, VertexInputState,
};
use sdl3::pixels::Color;
use sdl3::video::Window;

use crate::buffer::RgbaBuffer;
use crate::gpu_aa::{blit_regions, FxaaParams};
use crate::presentation::ScreenGeometry;
use crate::render_scale::FilterMode;

/// Vertex shader of the fullscreen FXAA triangle (compiled from `fxaa.vert`).
const FXAA_VERTEX_SPIRV: &[u8] = include_bytes!("../shaders/fxaa.vert.spv");
/// Fragment shader with the FXAA pass (compiled from `fxaa.frag`).
const FXAA_FRAGMENT_SPIRV: &[u8] = include_bytes!("../shaders/fxaa.frag.spv");

/// Format of the upload texture and of the FXAA render target.
///
/// RGBA8 is what [`RgbaBuffer`] holds, so the upload is a straight copy and the
/// FXAA pass works in the same 8-bit non-linear space the game was drawn in
/// (the historical frames are not linearised either).
const FRAME_FORMAT: TextureFormat = TextureFormat::R8g8b8a8Unorm;

/// A GPU anti-aliasing / presentation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuPresentError {
    stage: &'static str,
    message: String,
}

impl GpuPresentError {
    fn new(stage: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            stage,
            message: error.to_string(),
        }
    }

    /// The stage that failed (`device`, `shader`, `texture`, `blit`, …).
    pub fn stage(&self) -> &'static str {
        self.stage
    }

    /// The SDL message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for GpuPresentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SDL3 GPU ({}): {}", self.stage, self.message)
    }
}

impl std::error::Error for GpuPresentError {}

/// A texture of the presentation, cached for its size.
struct FrameTexture {
    width: u32,
    height: u32,
    texture: Texture<'static>,
}

impl FrameTexture {
    fn new(
        usage: TextureUsage,
        width: u32,
        height: u32,
        device: &Device,
    ) -> Result<Self, GpuPresentError> {
        let texture = device
            .create_texture(
                TextureCreateInfo::new()
                    .with_type(TextureType::_2D)
                    .with_format(FRAME_FORMAT)
                    .with_usage(usage)
                    .with_width(width)
                    .with_height(height)
                    .with_layer_count_or_depth(1)
                    .with_num_levels(1),
            )
            .map_err(|error| GpuPresentError::new("texture", error))?;

        Ok(Self {
            width,
            height,
            texture,
        })
    }

    fn matches(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }
}

/// The SDL3 GPU presentation of a window: FXAA pass + swapchain blit.
///
/// Owned by [`crate::SdlWindow`] and created lazily on the first frame with
/// anti-aliasing on, so `aa = Off` (and the whole pixel-perfect path) never
/// creates a device. Dropping it releases the device and the claimed window.
pub struct GpuPresenter {
    device: Device,
    pipeline: GraphicsPipeline,
    sampler: Sampler,
    /// The uploaded render frame (`SAMPLER`).
    source: Option<FrameTexture>,
    /// The FXAA render target (`COLOR_TARGET`).
    target: Option<FrameTexture>,
    /// Frames presented (diagnostics).
    frames: u64,
}

impl GpuPresenter {
    /// Creates a SPIR-V-only device, claims `window` and builds the FXAA pipeline.
    ///
    /// Every step is fallible: a machine without a Vulkan driver, a surface that
    /// cannot be claimed or a driver that rejects the shader returns
    /// [`GpuPresentError`], and the caller falls back to the Canvas path with
    /// `aa = Off` (owner decision: the fallback is visible, never silent).
    pub fn new(window: &Window) -> Result<Self, GpuPresentError> {
        // Debug builds enable SDL's GPU debug mode (validation messages).
        let device = Device::new(ShaderFormat::SPIRV, cfg!(debug_assertions))
            .map_err(|error| GpuPresentError::new("device", error))?;
        let device = device
            .with_window(window)
            .map_err(|error| GpuPresentError::new("window", error))?;

        let vertex = device
            .create_shader()
            .with_code(ShaderFormat::SPIRV, FXAA_VERTEX_SPIRV, ShaderStage::Vertex)
            .with_entrypoint(c"main")
            .build()
            .map_err(|error| GpuPresentError::new("vertex_shader", error))?;
        // One fragment sampler (set 2) and one uniform block (set 3).
        let fragment = device
            .create_shader()
            .with_code(
                ShaderFormat::SPIRV,
                FXAA_FRAGMENT_SPIRV,
                ShaderStage::Fragment,
            )
            .with_samplers(1)
            .with_uniform_buffers(1)
            .with_entrypoint(c"main")
            .build()
            .map_err(|error| GpuPresentError::new("fragment_shader", error))?;

        let pipeline = device
            .create_graphics_pipeline()
            .with_primitive_type(PrimitiveType::TriangleList)
            .with_vertex_shader(&vertex)
            .with_fragment_shader(&fragment)
            // The fullscreen triangle comes from `gl_VertexIndex`: no vertex buffer.
            .with_vertex_input_state(VertexInputState::new())
            .with_rasterizer_state(
                RasterizerState::new()
                    .with_fill_mode(FillMode::Fill)
                    .with_cull_mode(CullMode::None),
            )
            .with_target_info(
                GraphicsPipelineTargetInfo::new().with_color_target_descriptions(&[
                    ColorTargetDescription::new().with_format(FRAME_FORMAT),
                ]),
            )
            .build()
            .map_err(|error| GpuPresentError::new("pipeline", error))?;

        // The pipeline owns copies of the shaders from here on.
        drop(vertex);
        drop(fragment);

        // The FXAA taps are exact texels: nearest sampling keeps the pass
        // deterministic and stops the sampler from bleeding between neighbours
        // on flat areas. Clamp-to-edge keeps the border taps inside the frame.
        let sampler = device
            .create_sampler(
                SamplerCreateInfo::new()
                    .with_min_filter(Filter::Nearest)
                    .with_mag_filter(Filter::Nearest)
                    .with_mipmap_mode(SamplerMipmapMode::Nearest)
                    .with_address_mode_u(SamplerAddressMode::ClampToEdge)
                    .with_address_mode_v(SamplerAddressMode::ClampToEdge)
                    .with_address_mode_w(SamplerAddressMode::ClampToEdge),
            )
            .map_err(|error| GpuPresentError::new("sampler", error))?;

        Ok(Self {
            device,
            pipeline,
            sampler,
            source: None,
            target: None,
            frames: 0,
        })
    }

    /// Frames presented through the FXAA pass (diagnostics).
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The (re)allocated textures of a `width × height` frame.
    fn ensure_textures(&mut self, width: u32, height: u32) -> Result<(), GpuPresentError> {
        let stale = match (&self.source, &self.target) {
            (Some(source), Some(target)) => {
                !source.matches(width, height) || !target.matches(width, height)
            }
            _ => true,
        };

        if stale {
            self.source = Some(FrameTexture::new(
                TextureUsage::SAMPLER,
                width,
                height,
                &self.device,
            )?);
            // The FXAA target is blitted into the swapchain, and SDL requires
            // the *source* of a blit to carry the `SAMPLER` usage flag: a
            // `COLOR_TARGET`-only texture trips an assertion (and the blit is
            // dropped) on the Vulkan backend.
            self.target = Some(FrameTexture::new(
                TextureUsage::COLOR_TARGET | TextureUsage::SAMPLER,
                width,
                height,
                &self.device,
            )?);
        }

        Ok(())
    }

    /// Presents `buffer` through the FXAA pass into the `geometry` content rectangle.
    ///
    /// `filter` is the filter of the presentation mode (`Nearest` in
    /// pixel-perfect, `Linear` in HD) and applies to the **blit**, i.e. to the
    /// already anti-aliased frame — the same global choice the Canvas path uses
    /// (guardrail 5 of the remaster plan).
    pub fn present(
        &mut self,
        window: &Window,
        buffer: &RgbaBuffer,
        geometry: ScreenGeometry,
        filter: FilterMode,
    ) -> Result<(), GpuPresentError> {
        if buffer.is_empty() {
            return Ok(());
        }

        let (width, height) = (buffer.width(), buffer.height());

        self.ensure_textures(width, height)?;

        let mut command_buffer = self
            .device
            .acquire_command_buffer()
            .map_err(|error| GpuPresentError::new("command_buffer", error))?;

        // Nothing is visible (minimized/occluded window): skip the frame
        // entirely instead of uploading into a swapchain nobody sees.
        let Some(swapchain) = command_buffer
            .wait_and_acquire_swapchain_texture(window)
            .map_err(|error| GpuPresentError::new("swapchain", error))?
        else {
            command_buffer.cancel();

            return Ok(());
        };

        let (Some(source), Some(target)) = (&self.source, &self.target) else {
            command_buffer.cancel();

            return Err(GpuPresentError::new(
                "texture",
                "the source/target textures were not allocated",
            ));
        };

        self.upload(&command_buffer, buffer, source)?;
        self.fxaa_pass(&command_buffer, source, target)?;

        let swapchain_size = (swapchain.width(), swapchain.height());

        if let Some(regions) = blit_regions(geometry.content_rect(), swapchain_size) {
            let (sx, sy, sw, sh) = regions.source;
            let (dx, dy, dw, dh) = regions.destination;

            command_buffer.blit_texture(
                BlitInfo::default()
                    .with_source_texture(&target.texture)
                    .with_source_region(0, sx, sy, sw, sh)
                    .with_destination_texture(&swapchain)
                    .with_destination_region(0, dx, dy, dw, dh)
                    // The letterbox bars are cleared, the frame is written into
                    // its centred content rectangle.
                    .with_load_op(LoadOp::CLEAR)
                    .with_clear_color(Color::RGB(0, 0, 0))
                    .with_filter(match filter {
                        FilterMode::Nearest => Filter::Nearest,
                        FilterMode::Linear => Filter::Linear,
                    }),
            );
        }

        command_buffer
            .submit()
            .map_err(|error| GpuPresentError::new("submit", error))?;
        self.frames += 1;

        Ok(())
    }

    /// Copies the frame into the `SAMPLER` texture (one copy pass).
    fn upload(
        &self,
        command_buffer: &sdl3::gpu::CommandBuffer,
        buffer: &RgbaBuffer,
        source: &FrameTexture,
    ) -> Result<(), GpuPresentError> {
        let expected = buffer.width() as usize * buffer.height() as usize * 4;

        if buffer.pixels().len() < expected {
            return Err(GpuPresentError::new(
                "upload",
                format!(
                    "frame is {} bytes, expected {expected}",
                    buffer.pixels().len()
                ),
            ));
        }

        let transfer = self
            .device
            .create_transfer_buffer()
            .with_size(expected as u32)
            .with_usage(TransferBufferUsage::UPLOAD)
            .build()
            .map_err(|error| GpuPresentError::new("transfer_buffer", error))?;

        {
            let mut mapped = transfer.map::<u8>(&self.device, false);
            mapped
                .mem_mut()
                .copy_from_slice(&buffer.pixels()[..expected]);
            mapped.unmap();
        }

        let copy_pass = self
            .device
            .begin_copy_pass(command_buffer)
            .map_err(|error| GpuPresentError::new("copy_pass", error))?;

        copy_pass.upload_to_gpu_texture(
            TextureTransferInfo::new()
                .with_transfer_buffer(&transfer)
                .with_offset(0),
            TextureRegion::new()
                .with_texture(&source.texture)
                .with_layer(0)
                .with_width(source.width)
                .with_height(source.height)
                .with_depth(1),
            // The previous frame may still be sampling the texture: let SDL
            // rename it instead of waiting on the GPU.
            true,
        );
        self.device.end_copy_pass(copy_pass);

        Ok(())
    }

    /// One FXAA pass: fullscreen triangle from `source` into `target` (no depth).
    fn fxaa_pass(
        &self,
        command_buffer: &sdl3::gpu::CommandBuffer,
        source: &FrameTexture,
        target: &FrameTexture,
    ) -> Result<(), GpuPresentError> {
        let color_targets = [ColorTargetInfo::default()
            .with_texture(&target.texture)
            .with_load_op(LoadOp::CLEAR)
            .with_store_op(StoreOp::STORE)
            .with_clear_color(Color::RGB(0, 0, 0))];

        let render_pass = self
            .device
            .begin_render_pass(command_buffer, &color_targets, None)
            .map_err(|error| GpuPresentError::new("render_pass", error))?;

        render_pass.bind_graphics_pipeline(&self.pipeline);
        render_pass.bind_fragment_samplers(
            0,
            &[TextureSamplerBinding::new()
                .with_texture(&source.texture)
                .with_sampler(&self.sampler)],
        );
        command_buffer.push_fragment_uniform_data(0, &FxaaParams::new());

        // The fullscreen triangle: three vertices, no vertex buffer.
        render_pass.draw_primitives(3, 1, 0, 0);

        self.device.end_render_pass(render_pass);

        Ok(())
    }
}
