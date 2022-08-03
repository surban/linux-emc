// SPDX-License-Identifier: GPL-2.0

//! Firmware loading.
//!
//! C header: [`include/linux/i2c.h`](../../../../include/linux/firmware.h)
//!
//! Reference: <https://docs.kernel.org/driver-api/firmware/request_firmware.html>

use core::{ptr, slice};

use crate::{device::Device, error::Result, str::CStr, to_result};

/// Represents firmware data.
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Firmware {
    ptr: *const bindings::firmware,
}

impl Firmware {
    /// Send firmware request and wait for it.
    ///
    /// Should be called from user context where sleeping is allowed.
    ///
    /// `name` will be used as `$FIRMWARE` in the uevent environment and should be distinctive
    /// enough not to be confused with any other firmware image for this or any other device.
    ///
    /// The function can be called safely inside device’s suspend and resume callback.
    pub fn request(name: &CStr, device: &Device) -> Result<Self> {
        let mut ptr = ptr::null();
        to_result(unsafe { bindings::request_firmware(&mut ptr, name.as_char_ptr(), device.ptr) })?;
        Ok(unsafe { Self::new(ptr) })
    }

    /// Request for an optional firmware module.
    ///
    /// This function is similar in behaviour to [`request`](Self::request),
    /// except it doesn’t produce warning messages when the file is not found.
    pub fn request_nowarn(name: &CStr, device: &Device) -> Result<Self> {
        let mut ptr = ptr::null();
        to_result(unsafe {
            bindings::firmware_request_nowarn(&mut ptr, name.as_char_ptr(), device.ptr)
        })?;
        Ok(unsafe { Self::new(ptr) })
    }

    /// Load firmware directly without usermode helper.
    ///
    /// This function works pretty much like [`request`](Self::request), but this doesn’t fall back
    /// to usermode helper even if the firmware couldn’t be loaded directly from fs.
    pub fn request_direct(name: &CStr, device: &Device) -> Result<Self> {
        let mut ptr = ptr::null();
        to_result(unsafe {
            bindings::request_firmware_direct(&mut ptr, name.as_char_ptr(), device.ptr)
        })?;
        Ok(unsafe { Self::new(ptr) })
    }

    /// Creates a new firmware from the given pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid. Additionally ownership must be handed over.
    unsafe fn new(ptr: *const bindings::firmware) -> Self {
        // INVARIANT: The safety requirements of the function ensure the lifetime invariant.
        Self { ptr }
    }

    /// The firmware data.
    pub fn data(&self) -> &[u8] {
        unsafe { slice::from_raw_parts((*self.ptr).data, (*self.ptr).size) }
    }
}

impl Drop for Firmware {
    fn drop(&mut self) {
        unsafe { bindings::release_firmware(self.ptr) };
    }
}
