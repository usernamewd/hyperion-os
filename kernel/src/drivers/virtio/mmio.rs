//! Virtio over MMIO transport.
//!
//! The virtio-mmio register layout (virtio 1.1 §4.2.2) is a flat 64-byte
//! header followed by per-device-class config space starting at offset
//! 0x100. Every QEMU `virt` virtio slot follows this layout. We provide
//! a tiny [`Transport`] wrapper that exposes typed register reads /
//! writes and a uniform device-init protocol.

use core::ptr;

const REG_MAGIC_VALUE: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_VENDOR_ID: usize = 0x00c;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_INTERRUPT_STATUS: usize = 0x060;
const REG_INTERRUPT_ACK: usize = 0x064;
const REG_STATUS: usize = 0x070;
const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;
const REG_CONFIG: usize = 0x100;

const VIRTIO_MAGIC: u32 = 0x7472_6976; // "virt"

/// Virtio device-status bits.
pub const STATUS_ACK: u32 = 1;
pub const STATUS_DRIVER: u32 = 2;
pub const STATUS_DRIVER_OK: u32 = 4;
pub const STATUS_FEATURES_OK: u32 = 8;
pub const STATUS_FAILED: u32 = 128;

/// Common virtio feature bits.
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;

/// Device-id assignments (virtio 1.1 §5).
pub const VIRTIO_ID_NET: u32 = 1;
pub const VIRTIO_ID_BLOCK: u32 = 2;
pub const VIRTIO_ID_CONSOLE: u32 = 3;
pub const VIRTIO_ID_GPU: u32 = 16;

/// Probe header read from a virtio-mmio slot.
#[derive(Debug, Clone, Copy)]
pub struct ProbeHeader {
    pub device_id: u32,
    pub vendor_id: u32,
    pub version: u32,
}

#[derive(Clone, Copy)]
pub struct Transport {
    base: usize,
}

/// Errors returned by the transport-side init helpers.
#[derive(Debug)]
pub enum InitError {
    BadMagic,
    UnsupportedVersion,
    NoDevice,
    QueueTooSmall,
}

impl core::fmt::Display for InitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InitError::BadMagic => f.write_str("virtio: bad magic"),
            InitError::UnsupportedVersion => f.write_str("virtio: unsupported version"),
            InitError::NoDevice => f.write_str("virtio: no device in slot"),
            InitError::QueueTooSmall => f.write_str("virtio: queue too small"),
        }
    }
}

impl Transport {
    /// Inspect a virtio-mmio slot. Returns `Some(header)` if the slot
    /// holds a populated device, `None` otherwise.
    ///
    /// # Safety
    /// `base` must point at a 4 KiB region that is mapped MMIO and safe
    /// to read at the listed offsets.
    pub unsafe fn probe(base: usize) -> Option<ProbeHeader> {
        // SAFETY: caller guarantees `base` is mapped MMIO.
        let magic = unsafe { ptr::read_volatile((base + REG_MAGIC_VALUE) as *const u32) };
        if magic != VIRTIO_MAGIC {
            return None;
        }
        // SAFETY: as above.
        let device_id = unsafe { ptr::read_volatile((base + REG_DEVICE_ID) as *const u32) };
        if device_id == 0 {
            return None;
        }
        // SAFETY: as above.
        let version = unsafe { ptr::read_volatile((base + REG_VERSION) as *const u32) };
        let vendor_id = unsafe { ptr::read_volatile((base + REG_VENDOR_ID) as *const u32) };
        Some(ProbeHeader {
            device_id,
            vendor_id,
            version,
        })
    }

    /// Wrap a previously-probed slot.
    ///
    /// # Safety
    /// `base` must point at a populated virtio-mmio device.
    pub unsafe fn new(base: usize) -> Self {
        Self { base }
    }

    pub fn base(&self) -> usize {
        self.base
    }

    #[inline]
    fn r32(&self, off: usize) -> u32 {
        // SAFETY: base + off is mapped MMIO; reads have no side effects
        // for the registers we touch outside the explicit ack path.
        unsafe { ptr::read_volatile((self.base + off) as *const u32) }
    }

    #[inline]
    fn w32(&self, off: usize, v: u32) {
        // SAFETY: as above.
        unsafe { ptr::write_volatile((self.base + off) as *mut u32, v) }
    }

    /// Reset the device + run the initial portion of the virtio init
    /// dance: ACK + DRIVER bits set, version sanity-checked. Caller
    /// follows up with feature negotiation.
    pub fn init_begin(&self) -> Result<(), InitError> {
        if self.r32(REG_MAGIC_VALUE) != VIRTIO_MAGIC {
            return Err(InitError::BadMagic);
        }
        let version = self.r32(REG_VERSION);
        if version != 1 && version != 2 {
            return Err(InitError::UnsupportedVersion);
        }
        if self.r32(REG_DEVICE_ID) == 0 {
            return Err(InitError::NoDevice);
        }
        // Reset.
        self.w32(REG_STATUS, 0);
        self.w32(REG_STATUS, STATUS_ACK);
        self.w32(REG_STATUS, STATUS_ACK | STATUS_DRIVER);
        Ok(())
    }

    /// Read the 64-bit device-features register pair.
    pub fn read_features(&self) -> u64 {
        self.w32(REG_DEVICE_FEATURES_SEL, 0);
        let lo = self.r32(REG_DEVICE_FEATURES) as u64;
        self.w32(REG_DEVICE_FEATURES_SEL, 1);
        let hi = self.r32(REG_DEVICE_FEATURES) as u64;
        (hi << 32) | lo
    }

    /// Tell the device which 64-bit feature set we're requesting and
    /// flip FEATURES_OK in the status register.
    pub fn write_features(&self, features: u64) {
        self.w32(REG_DRIVER_FEATURES_SEL, 0);
        self.w32(REG_DRIVER_FEATURES, features as u32);
        self.w32(REG_DRIVER_FEATURES_SEL, 1);
        self.w32(REG_DRIVER_FEATURES, (features >> 32) as u32);

        let s = self.r32(REG_STATUS);
        self.w32(REG_STATUS, s | STATUS_FEATURES_OK);
    }

    /// Select a virtqueue and read its hardware-defined max size.
    pub fn queue_max(&self, queue_idx: u32) -> u32 {
        self.w32(REG_QUEUE_SEL, queue_idx);
        self.r32(REG_QUEUE_NUM_MAX)
    }

    /// Configure a virtqueue with descriptor / driver / device ring
    /// physical addresses and the chosen ring size, then mark it ready.
    pub fn queue_configure(
        &self,
        queue_idx: u32,
        size: u32,
        desc: u64,
        driver: u64,
        device: u64,
    ) {
        self.w32(REG_QUEUE_SEL, queue_idx);
        self.w32(REG_QUEUE_NUM, size);
        self.w32(REG_QUEUE_DESC_LOW, desc as u32);
        self.w32(REG_QUEUE_DESC_HIGH, (desc >> 32) as u32);
        self.w32(REG_QUEUE_DRIVER_LOW, driver as u32);
        self.w32(REG_QUEUE_DRIVER_HIGH, (driver >> 32) as u32);
        self.w32(REG_QUEUE_DEVICE_LOW, device as u32);
        self.w32(REG_QUEUE_DEVICE_HIGH, (device >> 32) as u32);
        self.w32(REG_QUEUE_READY, 1);
    }

    /// Notify the device that descriptors are available on `queue_idx`.
    pub fn queue_notify(&self, queue_idx: u32) {
        self.w32(REG_QUEUE_NOTIFY, queue_idx);
    }

    /// Flip DRIVER_OK in the status register; the device is now live.
    pub fn driver_ok(&self) {
        let s = self.r32(REG_STATUS);
        self.w32(REG_STATUS, s | STATUS_DRIVER_OK);
    }

    /// Read a u8 from device-class-specific config space.
    pub fn config_read_u8(&self, offset: usize) -> u8 {
        // SAFETY: REG_CONFIG + offset is within the device's MMIO frame
        // for any class we care about (header is 0x100 bytes, devices
        // have at most a few dozen config bytes).
        unsafe { ptr::read_volatile((self.base + REG_CONFIG + offset) as *const u8) }
    }

    /// Read a u32 from device-class-specific config space.
    pub fn config_read_u32(&self, offset: usize) -> u32 {
        // SAFETY: see above.
        unsafe { ptr::read_volatile((self.base + REG_CONFIG + offset) as *const u32) }
    }

    /// Read a u64 from device-class-specific config space.
    pub fn config_read_u64(&self, offset: usize) -> u64 {
        let lo = self.config_read_u32(offset) as u64;
        let hi = self.config_read_u32(offset + 4) as u64;
        (hi << 32) | lo
    }

    /// Read the interrupt status register.
    pub fn interrupt_status(&self) -> u32 {
        self.r32(REG_INTERRUPT_STATUS)
    }

    /// Acknowledge the bits set in `mask`.
    pub fn interrupt_ack(&self, mask: u32) {
        self.w32(REG_INTERRUPT_ACK, mask);
    }
}
