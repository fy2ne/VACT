//! DXGI Output Duplication — zero-copy desktop frame capture.
//!
//! # Why this exists
//!
//! Every other approach to desktop capture on Windows involves a painful tradeoff:
//!
//! - **GDI `BitBlt`**: works everywhere, but forces a full GPU→CPU readback on
//!   every call. At 1080p that's ~8 MB per frame through the PCI-e bus.
//! - **`PrintWindow` / `WM_PRINT`**: cooperative capture that misses overlays,
//!   hardware planes, and any window that doesn't handle the message correctly.
//! - **Windows.Graphics.Capture (WGC)**: modern and high-level, but adds a
//!   yellow border in older builds, requires a UWP-style activation flow, and
//!   yields `IDirect3DSurface` (WinRT) instead of raw `ID3D11Texture2D`.
//!
//! `IDXGIOutputDuplication` sidesteps all of this. The compositor hands us a
//! direct reference to the **shared VRAM surface** it just composited — no copy,
//! no readback, no driver round-trip. We only touch CPU memory when we
//! deliberately stage a frame for pixel inspection (e.g. OCR ROIs in V7).
//!
//! # Lifecycle
//!
//! ```text
//! DxgiCapture::new()
//!     └─ D3D11CreateDevice          (hardware adapter, no debug layer in prod)
//!     └─ IDXGIDevice::GetAdapter    (walk COM chain to reach DXGI)
//!     └─ IDXGIAdapter::EnumOutputs  (output 0 = primary monitor)
//!     └─ IDXGIOutput1::DuplicateOutput   (acquire the shared surface stream)
//!     └─ CreateTexture2D(STAGING)   (pre-allocate the CPU-readable buffer)
//!
//! loop {
//!     capture_frame()
//!         └─ AcquireNextFrame   → shared VRAM texture (≤ 16 ms on 60 Hz)
//!         └─ CopyResource       → stage into our CPU-readable texture
//!         └─ ReleaseFrame       → compositor can reclaim the surface immediately
//!         └─ Map / read / Unmap → BGRA8 pixel bytes into Vec<u8>
//! }
//! ```
//!
//! The staging texture is allocated once in `new()` and reused across every
//! frame. Allocating a 4K staging texture per frame would cost ~32 MB of VRAM
//! churn per second and trigger the driver's allocator on every iteration.

use std::time::Instant;

use windows::{
    core::{Interface, Result, HRESULT},
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11CreateDevice, D3D11_BIND_FLAG, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_RESOURCE_MISC_FLAG,
                D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
                ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
            },
            Dxgi::{
                IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
                DXGI_OUTDUPL_FRAME_INFO,
            },
            Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
        },
    },
};

// ── Sentinel HRESULTs ─────────────────────────────────────────────────────────
//
// These are not errors in the traditional sense — they are protocol signals
// from the duplication API that callers must handle explicitly.

/// The desktop surface has not changed since the last `AcquireNextFrame` call.
/// The correct response is a short sleep and a retry, not a crash.
pub const DXGI_ERROR_WAIT_TIMEOUT: HRESULT = HRESULT(0x887A0027_u32 as i32);

/// The GPU device was reset, or the session moved to a different desktop
/// (e.g. UAC prompt, lock screen, fast-user-switch). The `IDXGIOutputDuplication`
/// handle is now invalid and must be recreated from scratch.
pub const DXGI_ERROR_ACCESS_LOST: HRESULT = HRESULT(0x887A0026_u32 as i32);

// ── Public types ──────────────────────────────────────────────────────────────

/// A single desktop frame captured from the GPU compositor.
///
/// Pixel layout is **BGRA8, row-major** — exactly what DXGI gives us.
/// Callers that need RGB or RGBA must swap channels themselves; we deliberately
/// avoid paying that cost here since most pipeline stages (shaders, OCR) can
/// consume BGRA directly.
pub struct CapturedFrame {
    pub width:  u32,
    pub height: u32,

    /// Raw pixel bytes: `width * height * 4` bytes, B-G-R-A order.
    pub data: Vec<u8>,

    /// Wall-clock time from `AcquireNextFrame` to `Unmap`, in microseconds.
    /// This is the metric the V2 benchmark tracks.
    pub capture_us: u64,
}

/// An active DXGI desktop duplication session.
///
/// Holds the D3D11 device, the output duplication handle, and a pre-allocated
/// staging texture. All three must stay alive for the lifetime of the session —
/// dropping any one of them invalidates the others.
///
/// Thread-safety: `IDXGIOutputDuplication` is single-threaded by design.
/// Keep one `DxgiCapture` per thread and rotate captures if parallelism is needed.
#[allow(dead_code)] // device/adapter_name used from V3 onward
pub struct DxgiCapture {
    /// The D3D11 device we created. Kept alive so the DXGI chain doesn't drop.
    device:      ID3D11Device,
    /// Immediate context for `CopyResource` and `Map` / `Unmap` calls.
    context:     ID3D11DeviceContext,
    /// The live duplication handle. Invalidated on `DXGI_ERROR_ACCESS_LOST`.
    duplication: IDXGIOutputDuplication,
    /// CPU-readable staging texture. Same dimensions as the desktop.
    /// Allocated once; reused across all frames to avoid per-frame VRAM churn.
    staging:     ID3D11Texture2D,
    width:       u32,
    height:      u32,
    adapter_name: String,
}

impl DxgiCapture {
    /// Open a duplication session on the primary monitor (output index 0).
    ///
    /// # Errors
    ///
    /// - `E_ACCESSDENIED` (0x80070005): the calling process is not on the
    ///   interactive desktop (e.g. running inside a Windows service or a
    ///   non-interactive PowerShell job). Run from an interactive session.
    /// - `DXGI_ERROR_NOT_CURRENTLY_AVAILABLE`: another process holds exclusive
    ///   ownership of the duplication API (rare; usually clears on retry).
    pub fn new() -> Result<Self> {
        unsafe {
            // ── D3D11 device ──────────────────────────────────────────────────
            //
            // We ask for a plain hardware device with no special flags.
            // The debug layer (D3D11_CREATE_DEVICE_DEBUG) is intentionally
            // omitted in this path — it adds ~40 ms of startup overhead and
            // changes the threading model in ways that can mask real issues.
            let mut device:  Option<ID3D11Device>        = None;
            let mut context: Option<ID3D11DeviceContext> = None;

            D3D11CreateDevice(
                None,                        // adapter: None → use DXGI default
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),          // software rasteriser: not needed
                D3D11_CREATE_DEVICE_FLAG(0),
                None,                        // feature levels: let the driver pick
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,                        // feature level out: we don't care
                Some(&mut context),
            )?;

            let device  = device.unwrap();
            let context = context.unwrap();

            // ── Walk the DXGI adapter chain ───────────────────────────────────
            //
            // D3D11 and DXGI share the same underlying adapter object but expose
            // it through different COM interfaces. We QI from ID3D11Device to
            // IDXGIDevice and then climb to IDXGIAdapter. This ensures we're
            // duplicating the same GPU that our D3D device lives on — important
            // on multi-GPU systems where the iGPU may own the display outputs
            // while the dGPU handles compute.
            let dxgi_device: IDXGIDevice = device.cast()?;
            let adapter = dxgi_device.GetAdapter()?;

            let adapter_desc = adapter.GetDesc()?;
            let adapter_name = String::from_utf16_lossy(&adapter_desc.Description)
                .trim_end_matches('\0')
                .to_owned();

            // Output 0 is the primary monitor. Multi-monitor support (enumerating
            // all outputs and spawning one DxgiCapture per output) is deferred
            // to a later version when the scene graph needs to represent multiple
            // display spaces.
            let output = adapter.EnumOutputs(0)?;
            let output_desc = output.GetDesc()?;

            // DesktopCoordinates is in virtual-desktop space (DPI-unaware pixels).
            // For DPI-aware capture we would need IDXGIOutput6::GetDesc1, but
            // the staging texture dimensions must match the actual surface size,
            // which is always in physical pixels — so this is correct as-is.
            let rc     = output_desc.DesktopCoordinates;
            let width  = (rc.right  - rc.left) as u32;
            let height = (rc.bottom - rc.top)  as u32;

            // ── Acquire IDXGIOutputDuplication ────────────────────────────────
            //
            // This call fails with E_ACCESSDENIED if the process is not on the
            // interactive desktop. There is no workaround — it's an intentional
            // security boundary (prevents background services from screen-scraping
            // without the user's session context).
            let output1: IDXGIOutput1 = output.cast()?;
            let duplication = output1.DuplicateOutput(&device)?;

            // ── Pre-allocate the staging texture ──────────────────────────────
            //
            // D3D11_USAGE_STAGING with D3D11_CPU_ACCESS_READ is the only texture
            // configuration that supports Map(READ). It cannot be bound to the
            // pipeline (BindFlags must be 0), which is fine — we only use it as
            // a DMA target for CopyResource.
            //
            // windows-0.61: BindFlags / CPUAccessFlags / MiscFlags are raw u32
            // in the struct, so we unwrap the newtypes and cast from i32.
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width:     width,
                Height:    height,
                MipLevels: 1,
                ArraySize: 1,
                Format:    DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage:          D3D11_USAGE_STAGING,
                BindFlags:      D3D11_BIND_FLAG(0).0          as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0        as u32,
                MiscFlags:      D3D11_RESOURCE_MISC_FLAG(0).0  as u32,
            };
            let mut staging: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            let staging = staging.unwrap();

            log::info!("GPU adapter   : {}", adapter_name);
            log::info!("Output 0      : {}×{}  BGRA8", width, height);

            Ok(Self { device, context, duplication, staging, width, height, adapter_name })
        }
    }

    /// Returns the GPU adapter name (e.g. "NVIDIA GeForce RTX 4080").
    pub fn adapter_name(&self) -> &str { &self.adapter_name }

    /// Capture the next changed desktop frame.
    ///
    /// Blocks up to **100 ms** waiting for the compositor to signal a new frame.
    /// If the desktop hasn't changed within that window the call returns
    /// `Err(e)` where `e.code() == DXGI_ERROR_WAIT_TIMEOUT`. This is not a
    /// failure — callers should sleep briefly and retry.
    ///
    /// # Frame ownership and timing
    ///
    /// The order of operations here is deliberate:
    ///
    /// 1. `AcquireNextFrame` — compositor hands us a reference to its surface.
    /// 2. `CopyResource`     — GPU copies it into our staging texture. Still on
    ///                         the GPU timeline; returns immediately (async).
    /// 3. `ReleaseFrame`     — we release the compositor's surface *before*
    ///                         calling `Map`. This minimises the window where
    ///                         the compositor is blocked waiting for us.
    /// 4. `Map`              — stalls the CPU until the CopyResource GPU command
    ///                         has fully executed. Only then do we read pixels.
    /// 5. `Unmap`            — signals the driver that we're done with the
    ///                         CPU-visible mapping.
    pub fn capture_frame(&mut self) -> Result<CapturedFrame> {
        let t0 = Instant::now();

        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;

            for attempt in 0..8 {
                match self.duplication.AcquireNextFrame(150, &mut frame_info, &mut resource) {
                    Ok(_) => {
                        if resource.is_some() {
                            // If desktop hasn't presented its first surface yet on attempt 0, release and retry
                            if frame_info.LastPresentTime == 0 && attempt == 0 {
                                let _ = self.duplication.ReleaseFrame();
                                resource = None;
                                std::thread::sleep(std::time::Duration::from_millis(20));
                                continue;
                            }
                            break;
                        }
                    }
                    Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT && attempt < 7 => {
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                    Err(e) => return Err(e),
                }
            }

            let resource = match resource {
                Some(r) => r,
                None => return Err(windows::core::Error::new(DXGI_ERROR_WAIT_TIMEOUT, "DXGI wait timeout")),
            };
            let frame_tex: ID3D11Texture2D = resource.cast()?;

            // Upcast to ID3D11Resource for CopyResource / Map — both sides of
            // the copy must be the same interface type.
            let dst: ID3D11Resource = self.staging.cast()?;
            let src: ID3D11Resource = frame_tex.cast()?;

            self.context.CopyResource(&dst, &src);
            self.duplication.ReleaseFrame()?;

            // Map stalls until the GPU-side CopyResource completes.
            let mut mapped = Default::default();
            self.context.Map(&dst, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

            // GPU row pitch includes alignment padding to a power-of-two stride.
            // We strip that padding here so callers get a tightly-packed buffer.
            let pitch = mapped.RowPitch as usize;
            let w     = self.width  as usize;
            let h     = self.height as usize;
            let mut data = vec![0u8; w * h * 4];

            let src_ptr = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * h);
            for row in 0..h {
                data[row * w * 4..(row + 1) * w * 4]
                    .copy_from_slice(&src_ptr[row * pitch..row * pitch + w * 4]);
            }

            self.context.Unmap(&dst, 0);

            Ok(CapturedFrame {
                width:      self.width,
                height:     self.height,
                data,
                capture_us: t0.elapsed().as_micros() as u64,
            })
        }
    }
}
