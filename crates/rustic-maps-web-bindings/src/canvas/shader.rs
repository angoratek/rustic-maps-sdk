//! WGSL shader sources, the render pipelines and texture caches they feed,
//! and the vertex encoders that write their upload buffers.

use super::*;

/// WGSL pipeline of the textured tile quads, copied verbatim from the native
/// `rustic-maps-rendering` headless renderer: vertex positions arrive in clip space
/// and the fragment stage samples the tile texture.
const TILE_QUAD_SHADER_SOURCE: &str = "
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_coordinates: vec2<f32>,
    @location(1) opacity: f32,
};

@group(0)
@binding(0)
var tile_texture: texture_2d<f32>;

@group(0)
@binding(1)
var tile_sampler: sampler;

@vertex
fn vertex_main(@location(0) clip_position: vec4<f32>, @location(1) texture_coordinates: vec2<f32>, @location(2) opacity: f32) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = clip_position;
    output.texture_coordinates = texture_coordinates;
    output.opacity = opacity;
    return output;
}

@fragment
fn fragment_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(tile_texture, tile_sampler, vertex.texture_coordinates);
    return vec4<f32>(sampled.rgb, sampled.a * vertex.opacity);
}
";

/// WGSL pipeline of the solid-color overlay triangles, copied verbatim from
/// the native `rustic-maps-rendering` headless renderer's overlay layer: vertex
/// positions arrive already in clip space (the camera transform runs on the
/// CPU in `f64` in `rustic_maps_rendering::overlay_triangle_vertices`) and the
/// fragment stage writes the interpolated vertex color straight through.
const OVERLAY_SHADER_SOURCE: &str = "
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) rgba_color: vec4<f32>,
};

@vertex
fn vertex_main(@location(0) clip_position: vec4<f32>, @location(1) rgba_color: vec4<f32>) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = clip_position;
    output.rgba_color = rgba_color;
    return output;
}

@fragment
fn fragment_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    return vertex.rgba_color;
}
";

/// WGSL pipeline of the textured glyph quads, copied verbatim from the native
/// `rustic-maps-rendering` headless renderer: vertex positions arrive in clip space,
/// and the fragment stage thresholds the atlas' signed distance into an
/// anti-aliased coverage value tinted by the vertex color. The same pipeline
/// draws the halo pass (a lower threshold) and the text pass.
const GLYPH_ATLAS_SHADER_SOURCE: &str = "
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_coordinates: vec2<f32>,
    @location(1) rgba_color: vec4<f32>,
    @location(2) signed_distance_threshold: f32,
    @location(3) signed_distance_gamma: f32,
};

@group(0)
@binding(0)
var glyph_atlas: texture_2d<f32>;

@group(0)
@binding(1)
var glyph_sampler: sampler;

@vertex
fn vertex_main(
    @location(0) clip_position: vec4<f32>,
    @location(1) texture_coordinates: vec2<f32>,
    @location(2) rgba_color: vec4<f32>,
    @location(3) signed_distance_threshold: f32,
    @location(4) signed_distance_gamma: f32,
) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = clip_position;
    output.texture_coordinates = texture_coordinates;
    output.rgba_color = rgba_color;
    output.signed_distance_threshold = signed_distance_threshold;
    output.signed_distance_gamma = signed_distance_gamma;
    return output;
}

@fragment
fn fragment_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    let signed_distance = textureSample(glyph_atlas, glyph_sampler, vertex.texture_coordinates).r;
    let coverage = smoothstep(
        vertex.signed_distance_threshold - vertex.signed_distance_gamma,
        vertex.signed_distance_threshold + vertex.signed_distance_gamma,
        signed_distance);
    return vec4<f32>(vertex.rgba_color.rgb, vertex.rgba_color.a * coverage);
}
";

/// WGSL pipeline of the world-space overlay layer, drawn over the tiles from
/// geometry cached across frames. Vertices carry origin-relative normalized
/// world positions plus kind-specific screen-pixel parameters; the vertex
/// stage adds the frame uniform's view-projection offset, expands line widths,
/// joins, markers and pins in screen pixels (the CPU assembly's exact math),
/// and the fragment stage discards fragments outside the camera's own
/// world-copy window, so panning, rotation and zoom only rewrite the uniform.
const WORLD_SPACE_OVERLAY_SHADER_SOURCE: &str = "
struct FrameUniforms {
    anchor_clip_position: vec4<f32>,
    relative_view_projection: mat4x4<f32>,
    origin_relative_normalized: vec2<f32>,
    viewport_size_pixels: vec2<f32>,
    world_copy_offsets: array<vec4<f32>, 2>,
};

@group(0)
@binding(0)
var<uniform> frame: FrameUniforms;

const MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS: f32 = 1e-6;

struct VertexInput {
    @location(0) world_position: vec2<f32>,
    @location(1) reference_position: vec2<f32>,
    @location(2) next_position: vec2<f32>,
    @location(3) shape_kind: u32,
    @location(4) shape_parameters: vec2<f32>,
    @location(5) rgba_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) rgba_color: vec4<f32>,
    @location(1) window_relative_x: f32,
};

fn world_to_clip(position: vec2<f32>, copy_offset: f32) -> vec4<f32> {
    let relative = vec2<f32>(
        frame.origin_relative_normalized.x + position.x - copy_offset,
        frame.origin_relative_normalized.y - position.y,
    );
    return frame.anchor_clip_position
        + frame.relative_view_projection * vec4<f32>(relative, 0.0, 0.0);
}

fn clip_to_screen(clip_position: vec4<f32>) -> vec2<f32> {
    let normalized_device = clip_position.xy / clip_position.w;
    return vec2<f32>(
        (normalized_device.x + 1.0) * 0.5 * frame.viewport_size_pixels.x,
        (1.0 - (normalized_device.y + 1.0) * 0.5) * frame.viewport_size_pixels.y,
    );
}

fn screen_to_clip(screen_position: vec2<f32>, reference_clip_position: vec4<f32>) -> vec4<f32> {
    let normalized_device = vec2<f32>(
        2.0 * screen_position.x / frame.viewport_size_pixels.x - 1.0,
        1.0 - 2.0 * screen_position.y / frame.viewport_size_pixels.y,
    );
    return vec4<f32>(
        normalized_device * reference_clip_position.w,
        reference_clip_position.z,
        reference_clip_position.w,
    );
}

@vertex
fn vertex_main(vertex: VertexInput, @builtin(instance_index) instance_index: u32) -> VertexOutput {
    let copy_offset = frame.world_copy_offsets[instance_index / 4u][instance_index % 4u];
    var output: VertexOutput;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, -1.0);
    output.rgba_color = vertex.rgba_color;

    if (vertex.shape_kind == 0u) {
        // Direct ground geometry (filled polygon corners).
        output.clip_position = world_to_clip(vertex.world_position, copy_offset);
    } else if (vertex.shape_kind == 1u) {
        // Screen-facing anchor (markers and pins).
        let anchor_clip_position = world_to_clip(vertex.world_position, copy_offset);
        if (anchor_clip_position.w > 0.0) {
            let anchor_screen = clip_to_screen(anchor_clip_position);
            output.clip_position = screen_to_clip(
                anchor_screen + vertex.shape_parameters,
                anchor_clip_position,
            );
        }
    } else if (vertex.shape_kind == 2u) {
        // One endpoint of a screen-space-thickened segment.
        let self_clip_position = world_to_clip(vertex.world_position, copy_offset);
        let other_clip_position = world_to_clip(vertex.reference_position, copy_offset);
        if (self_clip_position.w > 0.0 && other_clip_position.w > 0.0) {
            let self_screen = clip_to_screen(self_clip_position);
            let other_screen = clip_to_screen(other_clip_position);
            let direction = (other_screen - self_screen) * vertex.shape_parameters.x;
            let direction_length = length(direction);
            if (direction_length >= MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS) {
                let perpendicular = vec2<f32>(-direction.y, direction.x) / direction_length;
                output.clip_position = screen_to_clip(
                    self_screen + perpendicular * vertex.shape_parameters.y,
                    self_clip_position,
                );
            }
        }
    } else if (vertex.shape_kind == 3u) {
        // One corner of a polyline bend's join square.
        let previous_clip_position = world_to_clip(vertex.reference_position, copy_offset);
        let vertex_clip_position = world_to_clip(vertex.world_position, copy_offset);
        let next_clip_position = world_to_clip(vertex.next_position, copy_offset);
        if (previous_clip_position.w > 0.0
            && vertex_clip_position.w > 0.0
            && next_clip_position.w > 0.0) {
            let previous_screen = clip_to_screen(previous_clip_position);
            let vertex_screen = clip_to_screen(vertex_clip_position);
            let next_screen = clip_to_screen(next_clip_position);
            let incoming = vertex_screen - previous_screen;
            let outgoing = next_screen - vertex_screen;
            let incoming_length = length(incoming);
            let outgoing_length = length(outgoing);
            if (incoming_length >= MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS
                && outgoing_length >= MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS) {
                let incoming_unit = incoming / incoming_length;
                let outgoing_unit = outgoing / outgoing_length;
                let direction_sum = incoming_unit + outgoing_unit;
                let direction_sum_length = length(direction_sum);
                var bisector = vec2<f32>(-incoming_unit.y, incoming_unit.x);
                if (direction_sum_length >= MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS) {
                    bisector = direction_sum / direction_sum_length;
                }
                let perpendicular = vec2<f32>(-bisector.y, bisector.x);
                let corner_screen = vertex_screen
                    + bisector * vertex.shape_parameters.x
                    + perpendicular * vertex.shape_parameters.y;
                output.clip_position = screen_to_clip(corner_screen, vertex_clip_position);
            }
        }
    }

    // The camera's own world-copy window: fragments recentered outside it
    // belong to an adjacent copy, which this frame draws as a separate
    // instance. The half-open range matches the CPU assembly's window rule.
    output.window_relative_x = frame.origin_relative_normalized.x
        + vertex.world_position.x
        - copy_offset;
    return output;
}

@fragment
fn fragment_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    if (vertex.window_relative_x < -0.5 || vertex.window_relative_x >= 0.5) {
        discard;
    }
    return vertex.rgba_color;
}
";

/// Floats in one world-space overlay frame uniform, matching the WGSL
/// `FrameUniforms` layout: four for the anchor clip position, sixteen for the
/// relative view-projection, two for the origin, two for the viewport, and
/// eight for the packed world-copy offsets.
pub(super) const WORLD_SPACE_OVERLAY_FRAME_UNIFORM_FLOATS: usize = 4 + 16 + 2 + 2 + 8;

/// Bytes of one world-space overlay frame uniform buffer.
pub(super) const WORLD_SPACE_OVERLAY_FRAME_UNIFORM_BYTES: u64 =
    (WORLD_SPACE_OVERLAY_FRAME_UNIFORM_FLOATS * std::mem::size_of::<f32>()) as u64;

/// The world-space overlay render pipeline and its frame uniform, drawn after
/// the tile quads and before the clip-space location indicator.
pub(super) struct WorldSpaceOverlayPipeline {
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) frame_uniform_buffer: wgpu::Buffer,
    pub(super) frame_uniform_bind_group: wgpu::BindGroup,
}

/// The world-space overlay geometry cached from the last overlay-content
/// revision: its draw extents and the content-defined origin the per-frame
/// uniform transforms to the camera. The vertices themselves live in
/// [`CanvasDrawState::world_space_overlay_vertices`], whose allocation the
/// cache reuses across revisions. The revision itself is tracked beside the
/// composed overlay list ([`CanvasDrawState::composed_overlay_revision`]).
pub(super) struct WorldSpaceOverlayCache {
    pub(super) vertex_count: u32,
    pub(super) triangle_count: u32,
    pub(super) origin_normalized_world: [f64; 2],
}

/// Alignment every upload payload and buffer size must observe: the
/// `wgpu` copy alignment, which `queue.write_buffer` enforces on both the
/// offset and the payload length.
pub(super) const UPLOAD_BUFFER_ALIGNMENT_BYTES: u64 = 4;

/// Factor a reusable upload buffer grows by when a payload outgrows it, so
/// repeated small growths cost a logarithmic number of allocations instead
/// of one per frame.
pub(super) const UPLOAD_BUFFER_GROWTH_FACTOR: u64 = 2;

/// A vertex or index buffer written from CPU-side bytes every frame. The
/// allocation is created on the first upload and grown only when a payload
/// outgrows its capacity, so a steady-state frame writes into an existing
/// buffer rather than creating one; the capacity is the high-water mark of
/// every payload the slot has carried.
#[derive(Default)]
pub(super) struct ReusableUploadBuffer {
    /// The allocation; absent until the first upload because `wgpu` buffers
    /// cannot be zero-sized.
    pub(super) buffer: Option<wgpu::Buffer>,
    /// Capacity of `buffer` in bytes, always a multiple of
    /// [`UPLOAD_BUFFER_ALIGNMENT_BYTES`].
    capacity_bytes: u64,
    /// How many allocations this slot has made, reported in the frame
    /// statistics so a steady frame's zero-allocation property stays
    /// observable.
    pub(super) allocations: usize,
}

impl ReusableUploadBuffer {
    /// Writes `contents` into the slot's buffer, growing the allocation when
    /// the payload no longer fits, and returns the buffer ready to bind.
    /// `label` and `usage` apply only when the allocation is created or
    /// replaced.
    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        usage: wgpu::BufferUsages,
        contents: &[u8],
    ) -> wgpu::Buffer {
        let required_bytes = rounded_up_to_upload_alignment(contents.len() as u64)
            .max(UPLOAD_BUFFER_ALIGNMENT_BYTES);
        let buffer = match &self.buffer {
            Some(buffer) if self.capacity_bytes >= required_bytes => buffer.clone(),
            _ => {
                let capacity_bytes = required_bytes.max(
                    self.capacity_bytes
                        .saturating_mul(UPLOAD_BUFFER_GROWTH_FACTOR),
                );
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: capacity_bytes,
                    usage: usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.buffer = Some(buffer.clone());
                self.capacity_bytes = capacity_bytes;
                self.allocations += 1;
                buffer
            }
        };
        // The encoders emit aligned payloads; only a future encoder handing
        // over a ragged tail needs the padding, and doing it here keeps the
        // write valid instead of failing validation.
        if contents.len() as u64 == required_bytes {
            queue.write_buffer(&buffer, 0, contents);
        } else {
            let mut padded_contents = contents.to_vec();
            padded_contents.resize(required_bytes as usize, 0);
            queue.write_buffer(&buffer, 0, &padded_contents);
        }
        buffer
    }
}

/// Rounds `bytes` up to the next multiple of [`UPLOAD_BUFFER_ALIGNMENT_BYTES`],
/// returning it unchanged when it already is one.
pub(super) fn rounded_up_to_upload_alignment(bytes: u64) -> u64 {
    let remainder = bytes % UPLOAD_BUFFER_ALIGNMENT_BYTES;
    if remainder == 0 {
        bytes
    } else {
        bytes + (UPLOAD_BUFFER_ALIGNMENT_BYTES - remainder)
    }
}

/// A GPU texture derived from a pooled payload, stamped with the pool
/// revision the payload held at upload so replaced or dropped payloads
/// regenerate their texture (see `rustic_maps_rendering::TileTexturePool`).
pub(super) struct TileTextureCacheEntry {
    pub(super) payload_revision: u64,
    pub(super) texture: Texture,
    pub(super) bind_group: wgpu::BindGroup,
}

/// One uploaded glyph atlas texture, stamped with the font's content
/// revision so the atlas uploads once per distinct font rather than per
/// frame. The bind group owns the texture view, which keeps the atlas alive.
pub(super) struct GlyphAtlasTextureCacheEntry {
    pub(super) revision: u64,
    pub(super) bind_group: wgpu::BindGroup,
}

/// One uploaded sprite atlas texture, stamped with the atlas' content
/// revision so the sheet uploads once per distinct sprite rather than per
/// frame. The bind group owns the texture view, which keeps the atlas alive.
pub(super) struct SpriteAtlasTextureCacheEntry {
    pub(super) revision: u64,
    pub(super) bind_group: wgpu::BindGroup,
}

/// Builds the textured-tile render pipeline and sampler on the device,
/// mirroring the native headless pipeline byte-for-byte.
pub(super) fn build_tile_pipeline(
    device: &wgpu::Device,
    surface_format: TextureFormat,
) -> Result<(wgpu::RenderPipeline, wgpu::Sampler), String> {
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-maps-web-bindings-tile-quad-shader"),
        source: wgpu::ShaderSource::Wgsl(TILE_QUAD_SHADER_SOURCE.into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rustic-maps-web-bindings-tile-bind-group-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let vertex_buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ClipSpaceTileVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: std::mem::offset_of!(ClipSpaceTileVertex, texture_coordinates)
                    as wgpu::BufferAddress,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32,
                offset: std::mem::offset_of!(ClipSpaceTileVertex, opacity) as wgpu::BufferAddress,
                shader_location: 2,
            },
        ],
    };

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-maps-web-bindings-tile-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let tile_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rustic-maps-web-bindings-tile-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader_module,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[Some(vertex_buffer_layout)],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader_module,
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let tile_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("rustic-maps-web-bindings-tile-sampler"),
        address_mode_u: TILE_TEXTURE_ADDRESS_MODE,
        address_mode_v: TILE_TEXTURE_ADDRESS_MODE,
        address_mode_w: TILE_TEXTURE_ADDRESS_MODE,
        mag_filter: TILE_TEXTURE_FILTER,
        min_filter: TILE_TEXTURE_FILTER,
        mipmap_filter: TILE_TEXTURE_MIPMAP_FILTER,
        ..Default::default()
    });

    Ok((tile_pipeline, tile_sampler))
}

/// Builds the solid-color overlay render pipeline on the device, mirroring
/// the native headless overlay pipeline byte-for-byte: the same pass-through
/// shader text, the same vertex layout (two `Float32x4` attributes — clip
/// position, then RGBA color), triangle-list primitives without culling, and
/// standard alpha blending so translucent overlay colors composite over the
/// base tiles.
pub(super) fn build_overlay_pipeline(
    device: &wgpu::Device,
    surface_format: TextureFormat,
) -> Result<wgpu::RenderPipeline, String> {
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-maps-web-bindings-overlay-shader"),
        source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER_SOURCE.into()),
    });

    let vertex_buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ClipSpaceOverlayVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: std::mem::offset_of!(ClipSpaceOverlayVertex, rgba_color)
                    as wgpu::BufferAddress,
                shader_location: 1,
            },
        ],
    };

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-maps-web-bindings-overlay-pipeline-layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });

    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rustic-maps-web-bindings-overlay-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_buffer_layout)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        }),
    )
}

/// Builds the textured-glyph render pipeline of the label layer on the
/// device: the native `rustic-maps-rendering` glyph shader and vertex layout verbatim
/// (clip position, atlas texture coordinates, paint and the signed-distance
/// threshold), triangle-list primitives without culling, and standard alpha
/// blending so halos and text composite over the base tiles.
pub(super) fn build_glyph_pipeline(
    device: &wgpu::Device,
    surface_format: TextureFormat,
) -> Result<wgpu::RenderPipeline, String> {
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-maps-web-bindings-glyph-atlas-shader"),
        source: wgpu::ShaderSource::Wgsl(GLYPH_ATLAS_SHADER_SOURCE.into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rustic-maps-web-bindings-glyph-atlas-bind-group-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let vertex_buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ClipSpaceGlyphVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: std::mem::offset_of!(ClipSpaceGlyphVertex, texture_coordinates)
                    as wgpu::BufferAddress,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: std::mem::offset_of!(ClipSpaceGlyphVertex, rgba_color)
                    as wgpu::BufferAddress,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32,
                offset: std::mem::offset_of!(ClipSpaceGlyphVertex, signed_distance_threshold)
                    as wgpu::BufferAddress,
                shader_location: 3,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32,
                offset: std::mem::offset_of!(ClipSpaceGlyphVertex, signed_distance_gamma)
                    as wgpu::BufferAddress,
                shader_location: 4,
            },
        ],
    };

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-maps-web-bindings-glyph-atlas-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rustic-maps-web-bindings-glyph-atlas-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_buffer_layout)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        }),
    )
}

/// Builds the glyph atlas sampler: clamp addressing over the single
/// `R8Unorm` page and linear filtering, mirroring the native glyph sampler.
pub(super) fn build_glyph_atlas_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("rustic-maps-web-bindings-glyph-atlas-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: GLYPH_ATLAS_TEXTURE_FILTER,
        min_filter: GLYPH_ATLAS_TEXTURE_FILTER,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// Builds the world-space overlay pipeline, its frame uniform buffer and the
/// bind group binding that uniform. The vertex layout mirrors
/// [`WorldSpaceOverlayVertex`]: two world offsets and a next offset, the shape
/// kind, two screen-pixel parameters, and the RGBA color.
pub(super) fn build_world_space_overlay_pipeline(
    device: &wgpu::Device,
    surface_format: TextureFormat,
) -> Result<WorldSpaceOverlayPipeline, String> {
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-maps-web-bindings-world-space-overlay-shader"),
        source: wgpu::ShaderSource::Wgsl(WORLD_SPACE_OVERLAY_SHADER_SOURCE.into()),
    });

    let vertex_buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<WorldSpaceOverlayVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: std::mem::offset_of!(WorldSpaceOverlayVertex, world_position)
                    as wgpu::BufferAddress,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: std::mem::offset_of!(WorldSpaceOverlayVertex, reference_position)
                    as wgpu::BufferAddress,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: std::mem::offset_of!(WorldSpaceOverlayVertex, next_position)
                    as wgpu::BufferAddress,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: std::mem::offset_of!(WorldSpaceOverlayVertex, shape_kind)
                    as wgpu::BufferAddress,
                shader_location: 3,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: std::mem::offset_of!(WorldSpaceOverlayVertex, shape_parameters)
                    as wgpu::BufferAddress,
                shader_location: 4,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: std::mem::offset_of!(WorldSpaceOverlayVertex, rgba_color)
                    as wgpu::BufferAddress,
                shader_location: 5,
            },
        ],
    };

    let frame_uniform_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rustic-maps-web-bindings-world-space-overlay-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

    let frame_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rustic-maps-web-bindings-world-space-overlay-uniform"),
        size: WORLD_SPACE_OVERLAY_FRAME_UNIFORM_BYTES,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let frame_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rustic-maps-web-bindings-world-space-overlay-bind-group"),
        layout: &frame_uniform_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: frame_uniform_buffer.as_entire_binding(),
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-maps-web-bindings-world-space-overlay-pipeline-layout"),
        bind_group_layouts: &[Some(&frame_uniform_bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rustic-maps-web-bindings-world-space-overlay-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader_module,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[Some(vertex_buffer_layout)],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader_module,
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    Ok(WorldSpaceOverlayPipeline {
        pipeline,
        frame_uniform_buffer,
        frame_uniform_bind_group,
    })
}

/// Serializes fragment vertices into the vertex and index byte streams the
/// GPU buffers expect: little-endian floats, [`CORNERS_PER_QUAD`] corners per
/// world-copy fragment, and the two-triangle index pattern `0, 1, 2, 2, 1, 3`
/// ([`INDICES_PER_QUAD`] per fragment) — a verbatim copy of the native
/// headless encoder (the caller bounds the fragment list below the 16-bit
/// index range first; on an out-of-range count the frame's `u16` indices
/// wrap rather than panic, exactly like the native encoder's own bound).
pub(super) fn encoded_fragment_buffers(
    fragment_vertices: &[[ClipSpaceTileVertex; 4]],
) -> (Vec<u8>, Vec<u8>) {
    let vertex_stride = std::mem::size_of::<ClipSpaceTileVertex>();
    let mut vertex_bytes =
        Vec::with_capacity(fragment_vertices.len() * vertex_stride * CORNERS_PER_QUAD);
    let mut index_bytes =
        Vec::with_capacity(fragment_vertices.len() * INDICES_PER_QUAD * std::mem::size_of::<u16>());
    for (fragment_index, vertices) in fragment_vertices.iter().enumerate() {
        let base_corner_index = u16::try_from(fragment_index * CORNERS_PER_QUAD).unwrap_or(0);
        for component in vertices.iter().flat_map(|vertex| {
            vertex
                .clip_position
                .iter()
                .chain(vertex.texture_coordinates.iter())
                .chain(std::iter::once(&vertex.opacity))
        }) {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        for index in [
            base_corner_index,
            base_corner_index + 1,
            base_corner_index + 2,
            base_corner_index + 2,
            base_corner_index + 1,
            base_corner_index + 3,
        ] {
            index_bytes.extend_from_slice(&index.to_le_bytes());
        }
    }
    (vertex_bytes, index_bytes)
}

/// The device pixels one CSS pixel covers on this canvas: the backing store
/// divided by the laid-out width. Reads the canvas' own ratio rather than
/// `window.devicePixelRatio` so a page that caps or scales the backing store
/// still draws text at the size it laid out.
pub(super) fn screen_pixel_ratio(canvas: &HtmlCanvasElement) -> f64 {
    let css_width = canvas.client_width();
    if css_width <= 0 {
        return 1.0;
    }
    let ratio = f64::from(canvas.width()) / f64::from(css_width);
    if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    }
}

/// Scales one styled polyline's CSS-pixel displacement to this canvas'
/// device pixels.
///
/// `scaled_to_device_pixel_ratio` scales a line's width and stroke sizes, but
/// a style's `line-translate` stays in CSS pixels. The camera-space assembly
/// the canvas calls receives a device ratio of one because the overlay list is
/// pre-scaled, so the displacement must take the ratio exactly once here; a 2x
/// canvas would otherwise draw a translated line at half its declared offset.
/// `translate_anchor` is left untouched, so the renderer still rotates a
/// map-anchored displacement with the bearing.
pub(super) fn overlay_scaled_to_device_pixel_ratio(
    overlay: &MapOverlay,
    device_pixel_ratio: f64,
) -> MapOverlay {
    let mut scaled_overlay = overlay.scaled_to_device_pixel_ratio(device_pixel_ratio);
    if let MapOverlay::RoutePolyline(route_polyline) = &mut scaled_overlay {
        route_polyline.translate_pixels[0] *= device_pixel_ratio;
        route_polyline.translate_pixels[1] *= device_pixel_ratio;
    }
    scaled_overlay
}

/// Parses one plain JavaScript overlay descriptor into its native
/// [`MapOverlay`], applying the documented rustic-maps-rendering default style where
/// the descriptor gives no override (the defaults are never re-stated here).
pub(super) fn validated_map_overlay(
    overlay_value: &JsValue,
    screen_ratio: f64,
) -> Result<MapOverlay, String> {
    let overlay_object: &Object = overlay_value
        .dyn_ref()
        .ok_or_else(|| "each overlay must be a plain JavaScript object".to_string())?;
    match required_javascript_string_field(overlay_object, OVERLAY_FIELD_KIND)?.as_str() {
        OVERLAY_KIND_ROUTE_POLYLINE => {
            let mut route_overlay =
                RoutePolylineOverlay::new(validated_javascript_coordinate_list(
                    &javascript_field(overlay_object, OVERLAY_FIELD_COORDINATES)?,
                    "the route polyline's coordinates",
                )?);
            if let Some(rgba_color) = optional_javascript_color(overlay_object)? {
                route_overlay.rgba_color = rgba_color;
            }
            if let Some(width_pixels) =
                optional_javascript_number_field(overlay_object, OVERLAY_FIELD_WIDTH_PIXELS)?
            {
                validated_positive_screen_pixels(width_pixels, OVERLAY_FIELD_WIDTH_PIXELS)?;
                route_overlay.width_pixels = width_pixels * screen_ratio;
            }
            Ok(MapOverlay::RoutePolyline(route_overlay))
        }
        OVERLAY_KIND_FILLED_POLYGON => {
            let mut polygon_overlay =
                FilledPolygonOverlay::new(validated_javascript_coordinate_list(
                    &javascript_field(overlay_object, OVERLAY_FIELD_RING_COORDINATES)?,
                    "the filled polygon's ring coordinates",
                )?);
            if let Some(rgba_color) = optional_javascript_color(overlay_object)? {
                polygon_overlay.rgba_color = rgba_color;
            }
            Ok(MapOverlay::FilledPolygon(polygon_overlay))
        }
        OVERLAY_KIND_POINT_MARKERS => {
            let mut markers_overlay =
                PointMarkersOverlay::new(validated_javascript_coordinate_list(
                    &javascript_field(overlay_object, OVERLAY_FIELD_POSITIONS)?,
                    "the point markers' positions",
                )?);
            if let Some(rgba_color) = optional_javascript_color(overlay_object)? {
                markers_overlay.rgba_color = rgba_color;
            }
            if let Some(radius_pixels) =
                optional_javascript_number_field(overlay_object, OVERLAY_FIELD_RADIUS_PIXELS)?
            {
                validated_positive_screen_pixels(radius_pixels, OVERLAY_FIELD_RADIUS_PIXELS)?;
                markers_overlay.radius_pixels = radius_pixels * screen_ratio;
            }
            Ok(MapOverlay::PointMarkers(markers_overlay))
        }
        OVERLAY_KIND_POINT_PINS => {
            let mut pins_overlay = PointPinsOverlay::new(validated_javascript_coordinate_list(
                &javascript_field(overlay_object, OVERLAY_FIELD_POSITIONS)?,
                "the point pins' positions",
            )?);
            if let Some(rgba_color) = optional_javascript_color(overlay_object)? {
                pins_overlay.rgba_color = rgba_color;
            }
            if let Some(radius_pixels) =
                optional_javascript_number_field(overlay_object, OVERLAY_FIELD_RADIUS_PIXELS)?
            {
                validated_positive_screen_pixels(radius_pixels, OVERLAY_FIELD_RADIUS_PIXELS)?;
                pins_overlay.radius_pixels = radius_pixels * screen_ratio;
            }
            Ok(MapOverlay::PointPins(pins_overlay))
        }
        OVERLAY_KIND_TRAFFIC_FLOW => {
            let segment_values = javascript_field(overlay_object, OVERLAY_FIELD_SEGMENTS)?;
            let segment_array: &Array = segment_values.dyn_ref().ok_or_else(|| {
                "the traffic flow's segments must be an array of segment objects".to_string()
            })?;
            let flows = (0..segment_array.length())
                .map(|segment_index| {
                    validated_traffic_flow_segment(&segment_array.get(segment_index))
                        .map_err(|message| format!("segment {segment_index}: {message}"))
                })
                .collect::<Result<Vec<TrafficFlowData>, String>>()?;
            let mut traffic_overlay = TrafficFlowOverlay::new(flows);
            if let Some(width_pixels) =
                optional_javascript_number_field(overlay_object, OVERLAY_FIELD_WIDTH_PIXELS)?
            {
                validated_positive_screen_pixels(width_pixels, OVERLAY_FIELD_WIDTH_PIXELS)?;
                traffic_overlay.width_pixels = width_pixels * screen_ratio;
            }
            Ok(MapOverlay::TrafficFlow(traffic_overlay))
        }
        unknown_kind => Err(format!(
            "unknown overlay kind {unknown_kind:?} (expected one of {:?}, {:?}, {:?}, {:?}, {:?})",
            OVERLAY_KIND_ROUTE_POLYLINE,
            OVERLAY_KIND_FILLED_POLYGON,
            OVERLAY_KIND_POINT_MARKERS,
            OVERLAY_KIND_POINT_PINS,
            OVERLAY_KIND_TRAFFIC_FLOW
        )),
    }
}

/// Parses one plain JavaScript traffic-flow segment object (its shape
/// polyline and congestion classification) into the native flow record; the
/// wasm seam passes the congestion bucket only, so the speed fields stay
/// `None` exactly like the native congestion-bucket constructor's.
pub(super) fn validated_traffic_flow_segment(
    segment_value: &JsValue,
) -> Result<TrafficFlowData, String> {
    let segment_object: &Object = segment_value
        .dyn_ref()
        .ok_or_else(|| "each traffic flow segment must be a plain JavaScript object".to_string())?;
    let shape_coordinates = validated_javascript_coordinate_list(
        &javascript_field(segment_object, OVERLAY_FIELD_SHAPE_COORDINATES)?,
        "the traffic flow segment's shape coordinates",
    )?;
    let congestion_level = validated_javascript_congestion_level(&javascript_field(
        segment_object,
        OVERLAY_FIELD_CONGESTION_LEVEL,
    )?)?;
    Ok(TrafficFlowData::from_congestion_level(
        shape_coordinates,
        congestion_level,
    ))
}

/// Reads a JavaScript congestion-level value back into its native
/// classification: inside a plain descriptor object the
/// [`CanvasTrafficFlowCongestionLevel`] enum value arrives as its plain
/// numeric discriminant (see the discriminant constants).
pub(super) fn validated_javascript_congestion_level(
    congestion_value: &JsValue,
) -> Result<TrafficFlowCongestionLevel, String> {
    let congestion_discriminant =
        validated_javascript_discriminant(congestion_value).map_err(|_rejection| {
            "the congestion level must be a CanvasTrafficFlowCongestionLevel value".to_string()
        })?;
    match congestion_discriminant {
        CONGESTION_LEVEL_FREE_FLOW_DISCRIMINANT => Ok(TrafficFlowCongestionLevel::FreeFlow),
        CONGESTION_LEVEL_SLOW_DISCRIMINANT => Ok(TrafficFlowCongestionLevel::Slow),
        CONGESTION_LEVEL_CONGESTED_DISCRIMINANT => Ok(TrafficFlowCongestionLevel::Congested),
        CONGESTION_LEVEL_BLOCKED_DISCRIMINANT => Ok(TrafficFlowCongestionLevel::Blocked),
        unknown_discriminant => Err(format!(
            "unknown CanvasTrafficFlowCongestionLevel value {unknown_discriminant}"
        )),
    }
}

/// Serializes the overlay triangle-list vertices into the vertex byte stream
/// the GPU buffer expects: little-endian floats — a verbatim copy of the
/// native headless encoder (the overlay draws non-indexed, a plain vertex
/// draw over the flat triangle list, so no index stream exists here either).
pub(super) fn encoded_glyph_vertices(glyph_vertices: &[ClipSpaceGlyphVertex]) -> Vec<u8> {
    let mut vertex_bytes = Vec::with_capacity(std::mem::size_of_val(glyph_vertices));
    for vertex in glyph_vertices {
        for component in &vertex.clip_position {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in &vertex.texture_coordinates {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in &vertex.rgba_color {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        vertex_bytes.extend_from_slice(&vertex.signed_distance_threshold.to_le_bytes());
        vertex_bytes.extend_from_slice(&vertex.signed_distance_gamma.to_le_bytes());
    }
    vertex_bytes
}

pub(super) fn encoded_overlay_vertices(overlay_vertices: &[ClipSpaceOverlayVertex]) -> Vec<u8> {
    let mut vertex_bytes = Vec::with_capacity(std::mem::size_of_val(overlay_vertices));
    for vertex in overlay_vertices {
        for component in vertex.clip_position.iter().chain(vertex.rgba_color.iter()) {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    vertex_bytes
}

/// Serializes the cached world-space overlay vertices into the byte stream
/// [`WorldSpaceOverlayVertex`]'s `repr(C)` layout expects: the three world
/// offsets, the shape kind as a little-endian `u32`, the two screen-pixel
/// parameters, and the RGBA color.
pub(super) fn encoded_world_space_overlay_vertices(
    vertices: &[WorldSpaceOverlayVertex],
) -> Vec<u8> {
    let mut vertex_bytes = Vec::with_capacity(vertices.len() * WorldSpaceOverlayVertex::BYTES);
    for vertex in vertices {
        for component in vertex
            .world_position
            .iter()
            .chain(vertex.reference_position.iter())
            .chain(vertex.next_position.iter())
        {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        vertex_bytes.extend_from_slice(&vertex.shape_kind.to_le_bytes());
        for component in vertex
            .shape_parameters
            .iter()
            .chain(vertex.rgba_color.iter())
        {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    vertex_bytes
}

/// Serializes one world-space overlay frame uniform into the WGSL
/// `FrameUniforms` layout: the anchor clip position, the column-major relative
/// view-projection, the origin-relative position, the viewport size and the
/// packed world-copy offsets.
pub(super) fn encoded_world_space_overlay_frame_uniforms(
    uniforms: &WorldSpaceOverlayFrameUniforms,
) -> Vec<u8> {
    let mut uniform_bytes = Vec::with_capacity(WORLD_SPACE_OVERLAY_FRAME_UNIFORM_FLOATS * 4);
    for component in uniforms.anchor_clip_position.iter() {
        uniform_bytes.extend_from_slice(&component.to_le_bytes());
    }
    for column in uniforms.relative_view_projection.iter() {
        for component in column.iter() {
            uniform_bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    for component in uniforms
        .origin_relative_normalized
        .iter()
        .chain(uniforms.viewport_size_pixels.iter())
    {
        uniform_bytes.extend_from_slice(&component.to_le_bytes());
    }
    for packed_offsets in uniforms.world_copy_offsets.iter() {
        for component in packed_offsets.iter() {
            uniform_bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    uniform_bytes
}
