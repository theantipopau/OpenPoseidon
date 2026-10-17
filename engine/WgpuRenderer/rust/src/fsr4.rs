// FSR 4 — AMD FidelityFX Super Resolution 4, on the DX12 backend.
//
// The FSR 4 network is AMD's signed binary, not ours. It runs through the FidelityFX API
// (FFX API) from the FidelityFX SDK's signedbin folder: amd_fidelityfx_loader_dx12.dll
// and amd_fidelityfx_upscaler_dx12.dll. The loader is opened at run time from
// WGR_FSR4_DIR, or beside the exe; the upscaler sits next to it. Nothing is linked and
// nothing is committed. The SDK licence governs those DLLs themselves.
//
// DX12 only. The shipped signed binary is DX12, and the SDK readme says Vulkan is not
// supported. The engine runs wgpu on Vulkan by default, so FSR 4 needs WGPU_BACKEND=dx12
// (Start.ps1 -Dx12). Off that backend the chain reports why and falls back.
//
// The FFI below mirrors the FidelityFX SDK 2.3.0 headers (api/include/ffx_api.h,
// api/include/ffx_api_types.h, upscalers/include/ffx_upscale.h, and
// api/include/dx12/ffx_api_dx12.h), MIT, Copyright (C) AMD. Layouts are C layouts; keep
// the field order in step with the headers.
//
// Frame contract. The chain has already recorded the input transitions into the scene
// encoder and submitted it, so the textures are in the states wgpu leaves them in. FFX
// is recorded into one raw D3D12 command list on the same queue wgpu submits to. Inputs
// are declared PIXEL|COMPUTE read and the output UNORDERED_ACCESS, so the raw list needs
// no barriers of its own. Allocators cycle behind a fence, so no allocator is reset while
// the GPU still runs it.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null;

use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue, ID3D12Device, ID3D12Fence,
    ID3D12GraphicsCommandList, ID3D12PipelineState, D3D12_COMMAND_LIST_TYPE_DIRECT,
    D3D12_FENCE_FLAG_NONE,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

use crate::upscaler::{self, TemporalUpscaler, UpscaleContext, UpscaleOutcome, UpscalerCaps};

// ---- FFX API constants (ffx_api.h, ffx_upscale.h, ffx_api_types.h) -------------------

// FFX_API_MAKE_EFFECT_SUB_ID(FFX_API_EFFECT_ID_UPSCALE, sub) and the DX12 backend sub-id.
const DESC_CREATE_UPSCALE: u64 = 0x0001_0000;
const DESC_CREATE_UPSCALE_VERSION: u64 = 0x0001_000B;
const DESC_CREATE_BACKEND_DX12: u64 = 0x0000_0002;
const DESC_DISPATCH_UPSCALE: u64 = 0x0001_0001;

// FFX_UPSCALER_MAKE_VERSION(4, 1, 1), the upscaler version the signed binary ships.
const UPSCALER_VERSION: u32 = (4 << 22) | (1 << 12) | 1;

// FfxApiCreateContextUpscaleFlags. Reversed depth and HDR input, render-size motion
// vectors without jitter, and our own exposure texture: the same contract as the DLSS
// backend (dlss.rs, DLSS_FLAG_*).
const CREATE_FLAG_HDR: u32 = 1 << 0;
const CREATE_FLAG_DEPTH_INVERTED: u32 = 1 << 3;

// FfxApiSurfaceFormat, by enum position in ffx_api_types.h.
const FORMAT_R16G16B16A16_FLOAT: u32 = 4;
const FORMAT_R16G16_FLOAT: u32 = 18;
const FORMAT_R8_UNORM: u32 = 25;
const FORMAT_R32_FLOAT: u32 = 28;

// FfxApiResourceType, FfxApiResourceUsage, FfxApiResourceState.
const RESOURCE_TYPE_TEXTURE2D: u32 = 2;
const USAGE_READ_ONLY: u32 = 0;
const USAGE_UAV: u32 = 1 << 1;
const STATE_UNORDERED_ACCESS: u32 = 1 << 1;
const STATE_PIXEL_COMPUTE_READ: u32 = (1 << 2) | (1 << 3);

const FFX_OK: u32 = 0;

// ---- C layouts ----------------------------------------------------------------------

#[repr(C)]
struct Header {
    ty: u64,
    next: *const Header,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Dim2 {
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Float2 {
    x: f32,
    y: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ResourceDesc {
    ty: u32,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    mip_count: u32,
    flags: u32,
    usage: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Resource {
    resource: *mut c_void,
    desc: ResourceDesc,
    state: u32,
}

impl Resource {
    const NULL: Resource = Resource {
        resource: std::ptr::null_mut(),
        desc: ResourceDesc {
            ty: 0,
            format: 0,
            width: 0,
            height: 0,
            depth: 0,
            mip_count: 0,
            flags: 0,
            usage: 0,
        },
        state: 0,
    };
}

#[repr(C)]
struct CreateUpscale {
    header: Header,
    flags: u32,
    max_render_size: Dim2,
    max_upscale_size: Dim2,
    fp_message: *const c_void,
}

#[repr(C)]
struct CreateUpscaleVersion {
    header: Header,
    version: u32,
}

#[repr(C)]
struct CreateBackendDx12 {
    header: Header,
    device: *mut c_void,
}

#[repr(C)]
struct DispatchUpscale {
    header: Header,
    command_list: *mut c_void,
    color: Resource,
    depth: Resource,
    motion_vectors: Resource,
    exposure: Resource,
    reactive: Resource,
    transparency_and_composition: Resource,
    output: Resource,
    jitter_offset: Float2,
    motion_vector_scale: Float2,
    render_size: Dim2,
    upscale_size: Dim2,
    enable_sharpening: bool,
    sharpness: f32,
    frame_time_delta: f32,
    pre_exposure: f32,
    reset: bool,
    camera_near: f32,
    camera_far: f32,
    camera_fov_vertical: f32,
    view_space_to_meters_factor: f32,
    flags: u32,
}

// ---- Runtime ------------------------------------------------------------------------

type CreateContextFn = unsafe extern "C" fn(*mut *mut c_void, *const Header, *const c_void) -> u32;
type DestroyContextFn = unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> u32;
// ffxDispatch takes a POINTER to the context handle (ffxContext*), like ffxDestroyContext.
type DispatchFn = unsafe extern "C" fn(*mut *mut c_void, *const Header) -> u32;

struct Runtime {
    create: CreateContextFn,
    destroy: DestroyContextFn,
    dispatch: DispatchFn,
    // Keeps the loader mapped for as long as the function pointers above can run.
    _lib: libloading::Library,
}

// WGR_FSR4_DIR, else the directory of the running exe.
fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WGR_FSR4_DIR") {
        return PathBuf::from(dir);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn load_runtime() -> Result<Runtime, String> {
    let dir = runtime_dir();
    let path = dir.join("amd_fidelityfx_loader_dx12.dll");
    if !path.is_file() {
        return Err(format!(
            "amd_fidelityfx_loader_dx12.dll not found in {} (set WGR_FSR4_DIR to the SDK signedbin folder)",
            dir.display()
        ));
    }
    // SAFETY: loading an AMD runtime DLL runs its DllMain; the file is the one the user
    // placed from the SDK. Nothing else is touched.
    let lib = unsafe { libloading::Library::new(&path) }
        .map_err(|e| format!("{}: {e}", path.display()))?;
    unsafe {
        let create = *lib
            .get::<CreateContextFn>(b"ffxCreateContext\0")
            .map_err(|e| format!("ffxCreateContext: {e}"))?;
        let destroy = *lib
            .get::<DestroyContextFn>(b"ffxDestroyContext\0")
            .map_err(|e| format!("ffxDestroyContext: {e}"))?;
        let dispatch = *lib
            .get::<DispatchFn>(b"ffxDispatch\0")
            .map_err(|e| format!("ffxDispatch: {e}"))?;
        Ok(Runtime {
            create,
            destroy,
            dispatch,
            _lib: lib,
        })
    }
}

// ---- Command ring -------------------------------------------------------------------

// Two allocators: one records while the other may still be executing. The fence value of
// each slot's last submission gates its reuse.
const RING: usize = 2;

struct Ring {
    queue: ID3D12CommandQueue,
    allocators: Vec<ID3D12CommandAllocator>,
    list: ID3D12GraphicsCommandList,
    fence: ID3D12Fence,
    event: HANDLE,
    slot_value: [u64; RING],
    next_value: u64,
    slot: usize,
}

impl Ring {
    fn new(device: &ID3D12Device, queue: &ID3D12CommandQueue) -> windows::core::Result<Self> {
        let mut allocators = Vec::with_capacity(RING);
        for _ in 0..RING {
            allocators.push(unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)? });
        }
        let list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocators[0],
                None::<&ID3D12PipelineState>,
            )?
        };
        // Created open; closed here so the first use can Reset it like every other frame.
        unsafe { list.Close()? };
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE)? };
        let event = unsafe { CreateEventW(None, false, false, None)? };
        Ok(Ring {
            queue: queue.clone(),
            allocators,
            list,
            fence,
            event,
            slot_value: [0; RING],
            next_value: 0,
            slot: 0,
        })
    }

    // Block until the GPU has passed `value`. Zero means the slot was never used.
    fn wait(&self, value: u64) -> windows::core::Result<()> {
        if value == 0 || unsafe { self.fence.GetCompletedValue() } >= value {
            return Ok(());
        }
        unsafe {
            self.fence.SetEventOnCompletion(value, self.event)?;
            WaitForSingleObject(self.event, INFINITE);
        }
        Ok(())
    }

    fn wait_all(&self) {
        let _ = self.wait(self.next_value);
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

// ---- Backend ------------------------------------------------------------------------

pub struct Fsr4Upscaler {
    // The adapter is on the DX12 backend. Decided once, at renderer creation.
    dx12: bool,
    runtime: Option<Runtime>,
    ring: Option<Ring>,
    // FFX context and the (render, output) size it was created for.
    ctx: *mut c_void,
    feature_key: Option<((u32, u32), (u32, u32))>,
    // Latched on the first hard failure; the chain then skips this backend.
    failed: bool,
}

// The FFX context and COM objects are used only by the render thread that owns the
// chain. The chain is moved into that thread once and never shared.
unsafe impl Send for Fsr4Upscaler {}

impl Fsr4Upscaler {
    pub fn new(dx12: bool) -> Self {
        Fsr4Upscaler {
            dx12,
            runtime: None,
            ring: None,
            ctx: std::ptr::null_mut(),
            feature_key: None,
            failed: false,
        }
    }

    fn disable(&mut self, msg: String) -> UpscaleOutcome {
        self.failed = true;
        UpscaleOutcome::Disabled(msg)
    }

    fn destroy_context(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        if let (Some(ring), Some(rt)) = (self.ring.as_ref(), self.runtime.as_ref()) {
            // The GPU may still be executing the last dispatch that used this context.
            ring.wait_all();
            unsafe { (rt.destroy)(&mut self.ctx, null()) };
        }
        self.ctx = std::ptr::null_mut();
        self.feature_key = None;
    }

    // Creates the FFX upscale context on the given D3D12 device. Chain order matters:
    // the upscale desc links to the version override, which links to the DX12 backend.
    fn create_context(
        rt: &Runtime,
        device: *mut c_void,
        render: (u32, u32),
        output: (u32, u32),
    ) -> Result<*mut c_void, u32> {
        let backend = CreateBackendDx12 {
            header: Header {
                ty: DESC_CREATE_BACKEND_DX12,
                next: null(),
            },
            device,
        };
        let version = CreateUpscaleVersion {
            header: Header {
                ty: DESC_CREATE_UPSCALE_VERSION,
                next: &backend.header,
            },
            version: UPSCALER_VERSION,
        };
        let desc = CreateUpscale {
            header: Header {
                ty: DESC_CREATE_UPSCALE,
                next: &version.header,
            },
            flags: CREATE_FLAG_HDR | CREATE_FLAG_DEPTH_INVERTED,
            max_render_size: Dim2 {
                width: render.0,
                height: render.1,
            },
            max_upscale_size: Dim2 {
                width: output.0,
                height: output.1,
            },
            fp_message: null(),
        };
        let mut ctx: *mut c_void = std::ptr::null_mut();
        let rc = unsafe { (rt.create)(&mut ctx, &desc.header, null()) };
        if rc == FFX_OK && !ctx.is_null() {
            Ok(ctx)
        } else {
            Err(rc)
        }
    }
}

impl Drop for Fsr4Upscaler {
    fn drop(&mut self) {
        self.destroy_context();
    }
}

// A texture as an FFX resource. The wgpu texture must stay alive for the dispatch; the
// caller holds it in the UpscaleContext for the whole evaluate call.
fn ffx_texture(
    tex: &wgpu::Texture,
    size: (u32, u32),
    format: u32,
    usage: u32,
    state: u32,
) -> Option<Resource> {
    let raw = unsafe {
        let hal = tex.as_hal::<wgpu::hal::api::Dx12>()?;
        hal.raw_resource().as_raw()
    };
    Some(Resource {
        resource: raw,
        desc: ResourceDesc {
            ty: RESOURCE_TYPE_TEXTURE2D,
            format,
            width: size.0,
            height: size.1,
            depth: 1,
            mip_count: 1,
            flags: 0,
            usage,
        },
        state,
    })
}

impl TemporalUpscaler for Fsr4Upscaler {
    fn label(&self) -> &'static str {
        "FSR 4"
    }

    fn caps(&self) -> UpscalerCaps {
        upscaler::FSR4_CAPS
    }

    fn evaluate(&mut self, ctx: &mut UpscaleContext) -> UpscaleOutcome {
        if self.failed || !ctx.settings.fsr4_on {
            return UpscaleOutcome::Skip;
        }
        if !self.dx12 {
            return self.disable(
                "FSR 4 needs the DX12 backend (start with WGPU_BACKEND=dx12, or Start.ps1 -Dx12)"
                    .into(),
            );
        }
        if !ctx.settings.temporal_enabled {
            return self.disable("the temporal path is off".into());
        }
        let render = ctx.render;
        let output = ctx.output;
        // SSAA territory: FSR 4 only upscales.
        if render.0 > output.0 || render.1 > output.1 {
            return UpscaleOutcome::Skip;
        }
        let (Some(color_tex), Some(depth), Some(velocity), Some(exposure)) = (
            ctx.color_tex.clone(),
            ctx.depth.as_ref(),
            ctx.velocity.as_ref(),
            ctx.exposure.as_ref(),
        ) else {
            return self.disable("missing color, depth, motion or exposure inputs".into());
        };
        if self.runtime.is_none() {
            match load_runtime() {
                Ok(rt) => {
                    ctx.notes.push(format!(
                        "FSR 4: runtime loaded from {}",
                        runtime_dir().display()
                    ));
                    self.runtime = Some(rt);
                }
                Err(e) => return self.disable(format!("runtime not loaded: {e}")),
            }
        }

        // The raw D3D12 device and queue wgpu itself submits to.
        let (device, queue) = {
            let Some(hal) = (unsafe { ctx.device.as_hal::<wgpu::hal::api::Dx12>() }) else {
                return self.disable("no DX12 device behind the wgpu device".into());
            };
            (hal.raw_device().clone(), hal.raw_queue().clone())
        };
        if self.ring.is_none() {
            match Ring::new(&device, &queue) {
                Ok(r) => self.ring = Some(r),
                Err(e) => return self.disable(format!("command ring: {e}")),
            }
        }

        // Context per size pair. A change tears down the old one after the GPU is done.
        let key = (render, output);
        let mut created = false;
        if self.feature_key != Some(key) || self.ctx.is_null() {
            self.destroy_context();
            let rt = self.runtime.as_ref().expect("runtime loaded above");
            match Self::create_context(rt, device.as_raw(), render, output) {
                Ok(c) => {
                    self.ctx = c;
                    self.feature_key = Some(key);
                    created = true;
                    ctx.notes.push(format!(
                        "FSR 4 context created: {}x{} -> {}x{} (FFX upscaler {}.{}.{})",
                        render.0,
                        render.1,
                        output.0,
                        output.1,
                        UPSCALER_VERSION >> 22,
                        (UPSCALER_VERSION >> 12) & 0x3FF,
                        UPSCALER_VERSION & 0xFFF
                    ));
                }
                Err(rc) => {
                    return self.disable(format!(
                        "ffxCreateContext returned {rc} (driver or FSR 4 runtime too old, or the GPU is not RDNA 4)"
                    ))
                }
            }
        }

        // Transitions into the scene encoder first, as the DLSS backend does, so the input
        // textures are in their read states before the raw list runs.
        ctx.encoder.transition_resources(
            std::iter::empty(),
            [
                wgpu::TextureTransition {
                    texture: &color_tex,
                    selector: None,
                    state: wgpu::TextureUses::RESOURCE,
                },
                wgpu::TextureTransition {
                    texture: &depth.0,
                    selector: None,
                    state: wgpu::TextureUses::RESOURCE,
                },
                wgpu::TextureTransition {
                    texture: &velocity.0,
                    selector: None,
                    state: wgpu::TextureUses::RESOURCE,
                },
                wgpu::TextureTransition {
                    texture: &exposure.0,
                    selector: None,
                    state: wgpu::TextureUses::RESOURCE,
                },
                wgpu::TextureTransition {
                    texture: &ctx.output_tex,
                    selector: None,
                    state: wgpu::TextureUses::STORAGE_READ_WRITE,
                },
            ]
            .into_iter(),
        );
        if let Some((hc_tex, _)) = ctx.history_control.as_ref() {
            ctx.encoder.transition_resources(
                std::iter::empty(),
                std::iter::once(wgpu::TextureTransition {
                    texture: hc_tex,
                    selector: None,
                    state: wgpu::TextureUses::RESOURCE,
                }),
            );
        }

        let Some(color_r) = ffx_texture(&color_tex, render, FORMAT_R16G16B16A16_FLOAT, USAGE_READ_ONLY, STATE_PIXEL_COMPUTE_READ) else {
            return self.disable("color texture is not a DX12 resource".into());
        };
        let Some(depth_r) = ffx_texture(&depth.0, render, FORMAT_R32_FLOAT, USAGE_READ_ONLY, STATE_PIXEL_COMPUTE_READ) else {
            return self.disable("depth texture is not a DX12 resource".into());
        };
        let Some(motion_r) = ffx_texture(&velocity.0, render, FORMAT_R16G16_FLOAT, USAGE_READ_ONLY, STATE_PIXEL_COMPUTE_READ) else {
            return self.disable("motion texture is not a DX12 resource".into());
        };
        let Some(exposure_r) = ffx_texture(&exposure.0, (1, 1), FORMAT_R32_FLOAT, USAGE_READ_ONLY, STATE_PIXEL_COMPUTE_READ) else {
            return self.disable("exposure texture is not a DX12 resource".into());
        };
        let Some(output_r) = ffx_texture(&ctx.output_tex, output, FORMAT_R16G16B16A16_FLOAT, USAGE_UAV, STATE_UNORDERED_ACCESS) else {
            return self.disable("output texture is not a DX12 resource".into());
        };
        // Engine history control (0..1, higher = trust history less) is the reactive mask
        // in FFX terms: the same intent, so it feeds the same input.
        let reactive_r = ctx
            .history_control
            .as_ref()
            .and_then(|(t, _)| ffx_texture(t, render, FORMAT_R8_UNORM, USAGE_READ_ONLY, STATE_PIXEL_COMPUTE_READ))
            .unwrap_or(Resource::NULL);

        // Same jitter and motion-vector conventions as the DLSS backend, so the two
        // share the dev-panel flips.
        let jy = ctx.settings.jitter_y_flip;
        let j = ctx.jitter_px;
        let jitter = [j[0], if jy { -j[1] } else { j[1] }];
        let mv_sign = if ctx.settings.mv_flip { -1.0f32 } else { 1.0 };
        let (mvw, mvh) = if ctx.settings.mv_render_space { render } else { output };
        let mv_scale = [mvw as f32 * mv_sign, mvh as f32 * mv_sign];
        // The engine's sharpness is RCAS stops (0 = sharpest, 2 = mildest); FFX's is 0..1.
        let sharpness = (1.0 - ctx.settings.fsr_sharpness * 0.5).clamp(0.0, 1.0);

        // Frame split, as the DLSS backend does. The scene so far submits, and the raw list
        // runs on the same queue right after it. The caller continues on a fresh encoder.
        let scene_encoder = std::mem::replace(
            ctx.encoder,
            ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wgr_frame_post_fsr4"),
            }),
        );
        ctx.queue.submit([scene_encoder.finish()]);

        let Some(ring) = self.ring.as_mut() else {
            return self.disable("command ring missing".into());
        };
        let rt = self.runtime.as_ref().expect("runtime loaded above");
        let slot = ring.slot;
        if let Err(e) = ring.wait(ring.slot_value[slot]) {
            return self.disable(format!("fence wait: {e}"));
        }
        if let Err(e) = unsafe { ring.allocators[slot].Reset() } {
            return self.disable(format!("allocator reset: {e}"));
        }
        if let Err(e) = unsafe { ring.list.Reset(&ring.allocators[slot], None::<&ID3D12PipelineState>) } {
            return self.disable(format!("command list reset: {e}"));
        }

        let desc = DispatchUpscale {
            header: Header {
                ty: DESC_DISPATCH_UPSCALE,
                next: null(),
            },
            command_list: ring.list.as_raw(),
            color: color_r,
            depth: depth_r,
            motion_vectors: motion_r,
            exposure: exposure_r,
            reactive: reactive_r,
            transparency_and_composition: Resource::NULL,
            output: output_r,
            jitter_offset: Float2 {
                x: jitter[0],
                y: jitter[1],
            },
            motion_vector_scale: Float2 {
                x: mv_scale[0],
                y: mv_scale[1],
            },
            render_size: Dim2 {
                width: render.0,
                height: render.1,
            },
            upscale_size: Dim2 {
                width: output.0,
                height: output.1,
            },
            enable_sharpening: true,
            sharpness,
            // The engine does not hand the backend a frame time yet; 60 Hz is the
            // history-accumulation assumption until it does.
            frame_time_delta: 1000.0 / 60.0,
            pre_exposure: ctx.settings.exposure_scale,
            reset: ctx.reset || created,
            camera_near: ctx.camera.near,
            camera_far: ctx.camera.far,
            camera_fov_vertical: ctx.camera.fov_y,
            view_space_to_meters_factor: 1.0,
            flags: 0,
        };
        let rc = unsafe { (rt.dispatch)(&mut self.ctx, &desc.header) };
        if rc != FFX_OK {
            // Nothing is submitted for this slot; the allocator is simply recycled next time.
            let _ = unsafe { ring.list.Close() };
            return self.disable(format!("ffxDispatch returned {rc}"));
        }
        if let Err(e) = unsafe { ring.list.Close() } {
            return self.disable(format!("command list close: {e}"));
        }
        let list: ID3D12CommandList = match ring.list.cast() {
            Ok(l) => l,
            Err(e) => return self.disable(format!("command list cast: {e}")),
        };
        unsafe { ring.queue.ExecuteCommandLists(&[Some(list)]) };
        ring.next_value += 1;
        if let Err(e) = unsafe { ring.queue.Signal(&ring.fence, ring.next_value) } {
            return self.disable(format!("fence signal: {e}"));
        }
        ring.slot_value[slot] = ring.next_value;
        ring.slot = (slot + 1) % RING;

        UpscaleOutcome::Done { quality: -1 }
    }
}
