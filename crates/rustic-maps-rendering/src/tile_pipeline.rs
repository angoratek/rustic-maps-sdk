//! The one GPU pipeline every host draws tile quads with.
//!
//! The native [`headless`](crate::headless) and [`surface`](crate::surface)
//! renderers and the WebAssembly canvas bind the same WGSL, the same vertex
//! layout, the same alpha-blending target state and the same sampler. Each
//! host keeps only its own upload and draw methods; the pipeline definition
//! lives here so the three cannot drift apart.
//!
//! Vertex positions arrive already in clip space (the camera transform runs on
//! the CPU in `f64`; see
//! [`Renderer::tile_vertices_in_clip_space`](crate::renderer::Renderer::tile_vertices_in_clip_space)),
//! so the vertex stage is a pass-through and the fragment stage samples the
//! tile texture, multiplying its alpha by the quad's opacity.

use crate::renderer::ClipSpaceTileVertex;

/// Filter mode of the tile sampler. Linear interpolation gives smooth
/// minification for both the pitched camera and fallback draws scaled up.
pub const TILE_TEXTURE_FILTER: wgpu::FilterMode = wgpu::FilterMode::Linear;

/// Interpolation between mip levels. The uploaded textures carry no mip
/// chain, so the mode never applies; linear keeps the sampler consistent
/// should mip chains be added.
pub const TILE_TEXTURE_MIPMAP_FILTER: wgpu::MipmapFilterMode = wgpu::MipmapFilterMode::Linear;

/// Clamp sampler addressing: quads sample exactly their declared sub-window
/// of the texture, so edge behavior never triggers.
pub const TILE_TEXTURE_ADDRESS_MODE: wgpu::AddressMode = wgpu::AddressMode::ClampToEdge;

/// The textured-quad shader shared by every host.
pub const TILE_QUAD_SHADER_SOURCE: &str = "
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

/// Builds the tile-quad render pipeline over `surface_format`.
///
/// `label_prefix` names the shader, bind-group layout, pipeline layout and
/// pipeline objects the host creates, so a validation error names the host
/// that built them; it changes no state.
pub fn create_tile_render_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    label_prefix: &str,
) -> wgpu::RenderPipeline {
    let shader_label = format!("{label_prefix}-tile-quad-shader");
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&shader_label),
        source: wgpu::ShaderSource::Wgsl(TILE_QUAD_SHADER_SOURCE.into()),
    });

    let bind_group_layout_label = format!("{label_prefix}-tile-bind-group-layout");
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&bind_group_layout_label),
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

    let pipeline_layout_label = format!("{label_prefix}-tile-pipeline-layout");
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&pipeline_layout_label),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline_label = format!("{label_prefix}-tile-pipeline");
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&pipeline_label),
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
    })
}

/// Builds the tile sampler: clamp addressing and linear filtering, the modes
/// every host paints with.
pub fn create_tile_sampler(device: &wgpu::Device, label_prefix: &str) -> wgpu::Sampler {
    let sampler_label = format!("{label_prefix}-tile-sampler");
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(&sampler_label),
        address_mode_u: TILE_TEXTURE_ADDRESS_MODE,
        address_mode_v: TILE_TEXTURE_ADDRESS_MODE,
        address_mode_w: TILE_TEXTURE_ADDRESS_MODE,
        mag_filter: TILE_TEXTURE_FILTER,
        min_filter: TILE_TEXTURE_FILTER,
        mipmap_filter: TILE_TEXTURE_MIPMAP_FILTER,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::TILE_QUAD_SHADER_SOURCE;

    /// The shader names exactly the two entry points the pipeline requests and
    /// the two bindings the layout declares; a rename on either side would
    /// otherwise fail only at device creation.
    #[test]
    fn the_shader_declares_the_entries_and_bindings_the_pipeline_uses() {
        for entry_point in ["fn vertex_main", "fn fragment_main"] {
            assert!(
                TILE_QUAD_SHADER_SOURCE.contains(entry_point),
                "the tile shader must declare {entry_point}"
            );
        }
        for binding in ["@binding(0)", "@binding(1)"] {
            assert!(
                TILE_QUAD_SHADER_SOURCE.contains(binding),
                "the tile shader must declare {binding}"
            );
        }
    }
}
