//! Virtio-gpu driver — 2D scanout flush.
//!
//! Implements the minimal slice of virtio-gpu (virtio 1.1 §5.7) needed
//! to show a real graphical window without firmware help. Flow:
//!
//! 1. ACK + DRIVER + negotiate `VIRTIO_F_VERSION_1`.
//! 2. Set up controlq (queue 0).
//! 3. `VIRTIO_GPU_CMD_GET_DISPLAY_INFO` → discover display 0's pixel
//!    dimensions.
//! 4. `RESOURCE_CREATE_2D` → allocate a B8G8R8X8 resource.
//! 5. `RESOURCE_ATTACH_BACKING` → point the resource at our heap-
//!    allocated framebuffer pixels.
//! 6. `SET_SCANOUT` → bind the resource to display 0.
//! 7. After every render: `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH`.
//!
//! The resulting framebuffer is registered with [`crate::display`] as
//! a hardware monitor so the compositor draws to the host display.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use super::mmio::{Transport, VIRTIO_F_VERSION_1};
use super::queue::{Virtqueue, VRING_DESC_F_NEXT, VRING_DESC_F_WRITE};
use crate::sync::Mutex;

const QUEUE_SIZE: u16 = 16;
const RESOURCE_ID: u32 = 1;
const SCANOUT_ID: u32 = 0;

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;

const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;

const FORMAT_B8G8R8X8_UNORM: u32 = 2;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CtrlHdr {
    ty: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct DisplayOne {
    r: Rect,
    enabled: u32,
    flags: u32,
}

#[repr(C)]
struct RespDisplayInfo {
    hdr: CtrlHdr,
    pmodes: [DisplayOne; 16],
}

#[repr(C)]
struct ResourceCreate2D {
    hdr: CtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct ResourceAttachBacking {
    hdr: CtrlHdr,
    resource_id: u32,
    nr_entries: u32,
}

#[repr(C)]
struct MemEntry {
    addr: u64,
    length: u32,
    padding: u32,
}

#[repr(C)]
struct SetScanout {
    hdr: CtrlHdr,
    r: Rect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
struct TransferToHost2D {
    hdr: CtrlHdr,
    r: Rect,
    offset: u64,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
struct ResourceFlush {
    hdr: CtrlHdr,
    r: Rect,
    resource_id: u32,
    padding: u32,
}

pub struct VirtioGpu {
    transport: Transport,
    queue: Mutex<Virtqueue>,
    width: u32,
    height: u32,
    fb: Mutex<Vec<u8>>,
}

impl VirtioGpu {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Push the entire framebuffer to the host display.
    pub fn flush(&self) {
        let _ = self.cmd_transfer_to_host_2d();
        let _ = self.cmd_resource_flush();
    }

    /// Borrow the host-side framebuffer for drawing. Holders of this
    /// slice may freely write pixels in B8G8R8X8 order; call
    /// [`Self::flush`] afterwards to publish them.
    pub fn with_pixels<R>(&self, f: impl FnOnce(&mut [u8], u32, u32) -> R) -> R {
        let mut fb = self.fb.lock();
        let w = self.width;
        let h = self.height;
        f(&mut fb, w, h)
    }

    fn cmd<Req, Resp>(&self, req: &Req, resp_size: usize) -> Vec<u8> {
        // Build a single 2-descriptor chain: req (read by device),
        // resp (written by device).
        let mut resp_buf = vec![0u8; resp_size];
        let req_addr = (req as *const Req) as u64;
        let req_len = core::mem::size_of::<Req>() as u32;
        let resp_addr = resp_buf.as_mut_ptr() as u64;
        let resp_len = resp_size as u32;

        let mut q = self.queue.lock();
        let head = q.alloc_chain(2).expect("virtqueue full");
        let mut idx = head;
        q.desc[idx as usize].addr = req_addr;
        q.desc[idx as usize].len = req_len;
        q.desc[idx as usize].flags = VRING_DESC_F_NEXT;
        idx = q.desc[idx as usize].next;
        q.desc[idx as usize].addr = resp_addr;
        q.desc[idx as usize].len = resp_len;
        q.desc[idx as usize].flags = VRING_DESC_F_WRITE;
        q.push_avail(head);
        drop(q);

        self.transport.queue_notify(0);

        let mut q = self.queue.lock();
        q.wait_used();
        q.free_chain(head);
        let _ = (Req::__phantom(), Resp::__phantom());
        resp_buf
    }

    fn cmd_set_scanout(&self) -> bool {
        let req = SetScanout {
            hdr: CtrlHdr {
                ty: VIRTIO_GPU_CMD_SET_SCANOUT,
                ..Default::default()
            },
            r: Rect {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
            scanout_id: SCANOUT_ID,
            resource_id: RESOURCE_ID,
        };
        let resp = self.cmd::<SetScanout, CtrlHdr>(&req, core::mem::size_of::<CtrlHdr>());
        resp_ok_nodata(&resp)
    }

    fn cmd_transfer_to_host_2d(&self) -> bool {
        let req = TransferToHost2D {
            hdr: CtrlHdr {
                ty: VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
                ..Default::default()
            },
            r: Rect {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
            offset: 0,
            resource_id: RESOURCE_ID,
            padding: 0,
        };
        let resp = self.cmd::<TransferToHost2D, CtrlHdr>(&req, core::mem::size_of::<CtrlHdr>());
        resp_ok_nodata(&resp)
    }

    fn cmd_resource_flush(&self) -> bool {
        let req = ResourceFlush {
            hdr: CtrlHdr {
                ty: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
                ..Default::default()
            },
            r: Rect {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
            resource_id: RESOURCE_ID,
            padding: 0,
        };
        let resp = self.cmd::<ResourceFlush, CtrlHdr>(&req, core::mem::size_of::<CtrlHdr>());
        resp_ok_nodata(&resp)
    }
}

/// Bring up a virtio-gpu device.
pub fn init(base: usize) -> Result<(), &'static str> {
    // SAFETY: caller probed the slot.
    let transport = unsafe { Transport::new(base) };
    transport.init_begin().map_err(|_| "init_begin failed")?;

    let device_feats = transport.read_features();
    transport.write_features(device_feats & VIRTIO_F_VERSION_1);

    let max = transport.queue_max(0);
    if max == 0 {
        return Err("virtio-gpu: no controlq");
    }
    let size = QUEUE_SIZE.min(max as u16);
    let q = Virtqueue::new(size);
    transport.queue_configure(0, size as u32, q.desc_phys(), q.avail_phys(), q.used_phys());

    transport.driver_ok();

    // Build a stub Self to use cmd() for GET_DISPLAY_INFO.
    let mut dev = VirtioGpu {
        transport,
        queue: Mutex::new(q),
        width: 0,
        height: 0,
        fb: Mutex::new(Vec::new()),
    };

    // 1. GET_DISPLAY_INFO
    let req = CtrlHdr {
        ty: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
        ..Default::default()
    };
    let resp = dev.cmd::<CtrlHdr, RespDisplayInfo>(&req, core::mem::size_of::<RespDisplayInfo>());
    if resp.len() < core::mem::size_of::<RespDisplayInfo>() {
        return Err("virtio-gpu: short response");
    }
    let info: &RespDisplayInfo =
        // SAFETY: response buffer is at least sizeof RespDisplayInfo.
        unsafe { &*(resp.as_ptr() as *const RespDisplayInfo) };
    if info.hdr.ty != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
        return Err("virtio-gpu: GET_DISPLAY_INFO failed");
    }
    let display0 = &info.pmodes[0];
    let width = if display0.r.width == 0 {
        1024
    } else {
        display0.r.width
    };
    let height = if display0.r.height == 0 {
        768
    } else {
        display0.r.height
    };
    dev.width = width;
    dev.height = height;
    let fb_bytes = (width as usize) * (height as usize) * 4;
    *dev.fb.lock() = vec![0u8; fb_bytes];

    // 2. RESOURCE_CREATE_2D
    let req = ResourceCreate2D {
        hdr: CtrlHdr {
            ty: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
            ..Default::default()
        },
        resource_id: RESOURCE_ID,
        format: FORMAT_B8G8R8X8_UNORM,
        width,
        height,
    };
    let resp = dev.cmd::<ResourceCreate2D, CtrlHdr>(&req, core::mem::size_of::<CtrlHdr>());
    if !resp_ok_nodata(&resp) {
        return Err("virtio-gpu: RESOURCE_CREATE_2D failed");
    }

    // 3. RESOURCE_ATTACH_BACKING — single-entry sg list pointing at our
    //    framebuffer.
    let entry = MemEntry {
        addr: dev.fb.lock().as_ptr() as u64,
        length: fb_bytes as u32,
        padding: 0,
    };
    let req = (
        ResourceAttachBacking {
            hdr: CtrlHdr {
                ty: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
                ..Default::default()
            },
            resource_id: RESOURCE_ID,
            nr_entries: 1,
        },
        entry,
    );
    let resp = dev.cmd::<(ResourceAttachBacking, MemEntry), CtrlHdr>(
        &req,
        core::mem::size_of::<CtrlHdr>(),
    );
    if !resp_ok_nodata(&resp) {
        return Err("virtio-gpu: RESOURCE_ATTACH_BACKING failed");
    }

    // 4. SET_SCANOUT
    if !dev.cmd_set_scanout() {
        return Err("virtio-gpu: SET_SCANOUT failed");
    }

    // Paint a recognisable test pattern + flush so users see something
    // immediately on `make run-gfx`.
    dev.with_pixels(|pixels, w, h| {
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 4;
                let r = (x * 255 / w as usize) as u8;
                let g = (y * 255 / h as usize) as u8;
                let b = ((x ^ y) & 0xff) as u8;
                pixels[i] = b;
                pixels[i + 1] = g;
                pixels[i + 2] = r;
                pixels[i + 3] = 0;
            }
        }
    });
    dev.flush();

    crate::log::info!("virtio-gpu: scanout 0 = {}x{} BGRX", width, height);

    let arc = Arc::new(dev);
    register_as_monitor(arc);
    Ok(())
}

fn resp_ok_nodata(resp: &[u8]) -> bool {
    if resp.len() < core::mem::size_of::<CtrlHdr>() {
        return false;
    }
    let ty = u32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]]);
    ty == VIRTIO_GPU_RESP_OK_NODATA
}

fn register_as_monitor(dev: Arc<VirtioGpu>) {
    use crate::display::{Framebuffer, Monitor, PixelFormat};
    let (w, h) = dev.dimensions();
    // We hand the compositor a Framebuffer that points at our heap
    // pixels, then drive flush() periodically. Holding `dev` alive is
    // the static.
    let fb_ptr = dev.fb.lock().as_mut_ptr();
    let stride = w * 4;
    // SAFETY: `fb_ptr` is a heap-owned buffer kept alive by GPU_DEV
    // below for the lifetime of the kernel. The compositor's monitor
    // reads/writes it under its own lock.
    let scanout = unsafe { Framebuffer::from_mmio(fb_ptr, w, h, stride, PixelFormat::Bgra8) };
    let monitor = Monitor::new_physical("virtio-gpu0", scanout);
    crate::display::register_monitor(monitor);

    // Stash the device so its heap framebuffer + queue stay alive.
    *GPU_DEV.lock() = Some(dev);
}

static GPU_DEV: Mutex<Option<Arc<VirtioGpu>>> = Mutex::new(None);

/// Return the registered virtio-gpu device, if any.
pub fn primary() -> Option<Arc<VirtioGpu>> {
    GPU_DEV.lock().clone()
}

// Tiny helper trait so `cmd::<Req, Resp>` carries phantom ownership.
trait __Phantom {
    fn __phantom() {}
}
impl<T> __Phantom for T {}
