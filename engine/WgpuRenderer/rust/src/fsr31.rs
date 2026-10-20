// FSR 3.1 — the cross-vendor temporal upscaler (RX 5000+, RTX 20+, Intel
// best-effort; Vulkan AND DX12), as a wgpu-native backend with no vendor SDK.
//
// Why this exists next to FSR 4 (fsr4.rs): FSR 4 is the best quality where it
// runs, but it needs the DX12 backend plus the signed loader/upscaler DLLs and
// (since SDK 2.3 / Upscaling 4.1.1) an RX 7000+ card. FSR 3.1 is MIT-licensed
// FidelityFX source that runs on the Vulkan path every build already ships, so
// it is the default AMD/NVIDIA answer. Chain order is DLSS > FSR 4 > FSR 3.1 >
// Native TAA (100% only) > FSR 1 > Bilinear.
//
// Accumulation core (FSR31-ACCUM): the preferred path consumes the agreed
// render-res accumulator (`Temporal::render_debug_resolve`, exposed as
// `ctx.native_history`) — reprojected along velocity, neighbourhood-clamped,
// blended 90/10 — then EASU-upscales that stable image with PLAIN viewport
// constants (the accumulator carries no jitter) plus RCAS. At render == output
// that is Native AA mode. Disocclusion rejection lives in the resolve's depth
// compare; this backend adds no second history of its own.
//
// Fallback (honest v1 scope): when no accumulator ran this frame (no velocity
// target, first frame before the resolve), jitter-compensated EASU + RCAS
// straight from the current frame through the same MIT `fsr1.wgsl` kernels
// FSR 1 uses. A real step past FSR 1, not a relabel: FSR 1 ignores jitter
// entirely, so under render scale with the temporal path on it upscales a
// jittered frame without un-jittering it and the output shimmers. This backend
// subtracts the frame's jitter (render-pixel units, the same convention
// `temporal.rs` feeds DLSS) from the EASU viewport mapping, so the upscaled
// image is stable.
//
// License note: the EASU/RCAS kernels are AMD FidelityFX MIT, Copyright (c) 2021
// Advanced Micro Devices, Inc. — see THIRD_PARTY_NOTICES.md ("FSR 3.1") and
// `fsr1.wgsl` for provenance. Only the jitter compensation here is ours.

use crate::upscaler::{self, TemporalUpscaler, UpscaleContext, UpscaleOutcome};

// EASU viewport constants for a jittered frame: the base mapping from
// `upscaler::easu_constants`, shifted by minus the frame's jitter so the sample
// grid lands where the un-jittered image would be. Pure, so the compensation is
// unit-testable without a device.
pub fn easu_constants_jittered(
    render: (u32, u32),
    output: (u32, u32),
    jitter_px: [f32; 2],
) -> [f32; 16] {
    let mut c = upscaler::easu_constants(render, output);
    c[2] -= jitter_px[0];
    c[3] -= jitter_px[1];
    c
}

pub struct Fsr31Upscaler {
    kit: Option<Fsr31Kit>,
    intermediate: Option<(wgpu::Texture, wgpu::TextureView)>,
}

// Mirrors `FsrUpscaler`'s kit (upscaler.rs) deliberately: the two backends must
// stay independently toggleable, and sharing the pipeline object would couple
// their lifetimes. Dedup into a shared EASU/RCAS helper is the follow-up once
// the temporal-accumulation core lands here.
struct Fsr31Kit {
    easu: wgpu::ComputePipeline,
    rcas: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    sampler: wgpu::Sampler,
}

impl Fsr31Upscaler {
    pub fn new() -> Self {
        Fsr31Upscaler {
            kit: None,
            intermediate: None,
        }
    }

    fn ensure_kit(&mut self, device: &wgpu::Device) {
        self.kit.get_or_insert_with(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("wgr_fsr31"),
                source: wgpu::ShaderSource::Wgsl(include_str!("fsr1.wgsl").into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("wgr_fsr31"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(80),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba16Float,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("wgr_fsr31"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let mk = |entry: &str| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pl),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
            };
            Fsr31Kit {
                easu: mk("easu"),
                rcas: mk("rcas"),
                layout,
                params: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("wgr_fsr31_params"),
                    size: 80,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("wgr_fsr31"),
                    address_mode_u: wgpu::AddressMode::ClampToEdge,
                    address_mode_v: wgpu::AddressMode::ClampToEdge,
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                }),
            }
        });
    }
}

impl TemporalUpscaler for Fsr31Upscaler {
    fn label(&self) -> &'static str {
        "FSR 3.1"
    }

    fn caps(&self) -> upscaler::UpscalerCaps {
        upscaler::FSR31_CAPS
    }

    fn evaluate(&mut self, ctx: &mut UpscaleContext) -> UpscaleOutcome {
        if !ctx.settings.fsr31_on {
            return UpscaleOutcome::Skip;
        }
        // Temporal contract: the accumulation core and the jitter compensation
        // both need the temporal path (jitter + velocity); without it there is
        // nothing this backend does that FSR 1 does not — skip and let the
        // spatial path own the frame.
        if !ctx.settings.temporal_enabled {
            return UpscaleOutcome::Skip;
        }
        // EASU only upscales; SSAA (render above output) stays with the bilinear
        // downsample. render == output is allowed: EASU is ~identity, the jitter
        // compensation still stabilises, and RCAS still buys its sharpening.
        if ctx.render.0 > ctx.output.0 || ctx.render.1 > ctx.output.1 {
            return UpscaleOutcome::Skip;
        }
        let device = ctx.device;
        let need_new = self
            .intermediate
            .as_ref()
            .map(|(t, _)| (t.width(), t.height()) != ctx.output)
            .unwrap_or(true);
        if need_new {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("wgr_fsr31_mid"),
                size: wgpu::Extent3d {
                    width: ctx.output.0,
                    height: ctx.output.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            self.intermediate = Some((t, v));
        }
        // FSR31-ACCUM: the accumulator is the stable, unjittered image — upscale
        // it with plain viewport constants. Without one, fall back to the
        // jitter-compensated single-frame path.
        let from_accum = ctx.native_history.is_some();
        let input_view: &wgpu::TextureView = match ctx.native_history.as_ref() {
            Some((_, hv)) => hv,
            None => &ctx.color_view,
        };
        let mut data = [0f32; 20];
        data[..16].copy_from_slice(&if from_accum {
            upscaler::easu_constants(ctx.render, ctx.output)
        } else {
            easu_constants_jittered(ctx.render, ctx.output, ctx.jitter_px)
        });
        data[16] = (-ctx.settings.fsr_sharpness.clamp(0.0, 2.0)).exp2();
        self.ensure_kit(device);
        let kit = self.kit.as_ref().expect("ensured above");
        ctx.queue
            .write_buffer(&kit.params, 0, bytemuck::cast_slice(&data));
        let (_, mid_view) = self.intermediate.as_ref().expect("ensured above");
        let easu_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wgr_fsr31_easu"),
            layout: &kit.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&kit.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: kit.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(mid_view),
                },
            ],
        });
        let rcas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wgr_fsr31_rcas"),
            layout: &kit.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(mid_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&kit.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: kit.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&ctx.output_view),
                },
            ],
        });
        let gx = ctx.output.0.div_ceil(8);
        let gy = ctx.output.1.div_ceil(8);
        ctx.encoder.push_debug_group("wgr_fsr31");
        {
            let mut cp = ctx
                .encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("wgr_fsr31_easu"),
                    timestamp_writes: None,
                });
            cp.set_pipeline(&kit.easu);
            cp.set_bind_group(0, &easu_bind, &[]);
            cp.dispatch_workgroups(gx, gy, 1);
        }
        {
            let mut cp = ctx
                .encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("wgr_fsr31_rcas"),
                    timestamp_writes: None,
                });
            cp.set_pipeline(&kit.rcas);
            cp.set_bind_group(0, &rcas_bind, &[]);
            cp.dispatch_workgroups(gx, gy, 1);
        }
        ctx.encoder.pop_debug_group();
        {
            static ONCE_ACCUM: std::sync::Once = std::sync::Once::new();
            static ONCE_SINGLE: std::sync::Once = std::sync::Once::new();
            let (rw, rh) = ctx.render;
            let (ow, oh) = ctx.output;
            if from_accum {
                ONCE_ACCUM.call_once(|| {
                    eprintln!(
                        "[wgr] FSR 3.1 active: {rw}x{rh} -> {ow}x{oh} (accumulator + EASU + RCAS)"
                    );
                });
            } else {
                ONCE_SINGLE.call_once(|| {
                    eprintln!(
                        "[wgr] FSR 3.1 active: {rw}x{rh} -> {ow}x{oh} (single-frame jitter-compensated EASU+RCAS, no accumulator)"
                    );
                });
            }
        }
        UpscaleOutcome::Done { quality: -1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_compensation_shifts_the_viewport_mapping() {
        let base = upscaler::easu_constants((1280, 720), (1920, 1080));
        let j = easu_constants_jittered((1280, 720), (1920, 1080), [0.25, -0.5]);
        // Only the viewport-offset lanes move, by exactly minus the jitter.
        assert!((j[2] - (base[2] - 0.25)).abs() < 1e-6, "x lane {}", j[2]);
        assert!((j[3] - (base[3] + 0.5)).abs() < 1e-6, "y lane {}", j[3]);
        for (i, (a, b)) in base.iter().zip(j.iter()).enumerate() {
            if i == 2 || i == 3 {
                continue;
            }
            assert_eq!(a, b, "lane {i} must be untouched");
        }
    }

    #[test]
    fn zero_jitter_is_plain_easu() {
        let base = upscaler::easu_constants((960, 540), (1920, 1080));
        assert_eq!(easu_constants_jittered((960, 540), (1920, 1080), [0.0, 0.0]), base);
    }

    #[test]
    fn caps_are_temporal_with_own_sharpening() {
        let caps = Fsr31Upscaler::new().caps();
        assert_eq!(caps, upscaler::FSR31_CAPS);
        assert_eq!(caps.info_id, upscaler::ID_FSR31);
        assert!(caps.needs_temporal_inputs);
        // RCAS is the backend's own second dispatch; external sharpening would double it.
        assert!(!caps.wants_external_sharpen);
        assert_eq!(Fsr31Upscaler::new().label(), "FSR 3.1");
    }
}
