// SPDX-License-Identifier: GPL-2.0

//! I2C devices.
//!
//! C header: [`include/linux/i2c.h`](../../../../include/linux/i2c.h)
//!
//! Reference: <https://docs.kernel.org/i2c/index.html>

#![allow(unused_imports)]
#![allow(dead_code)]

use core::ffi::c_void;

use crate::{
    bindings,
    device::{self, RawDevice},
    driver,
    error::{from_kernel_result, Result},
    of,
    str::{BStr, CStr},
    to_result,
    types::PointerWrapper,
    ThisModule,
};

/// A registration of an I2C driver.
pub type DriverRegistration<T> = driver::Registration<DriverAdapter<T>>;

/// An adapter for the registration of I2C drivers.
pub struct DriverAdapter<T: Driver>(T);

impl<T: Driver> driver::DriverOps for DriverAdapter<T> {
    type RegType = bindings::i2c_driver;

    unsafe fn register(
        reg: *mut bindings::i2c_driver,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        // SAFETY: By the safety requirements of this function (defined in the trait definition),
        // `reg` is non-null and valid.
        let pdrv = unsafe { &mut *reg };

        pdrv.driver.name = name.as_char_ptr();
        pdrv.probe_new = Some(Self::probe_new_callback);
        pdrv.remove = Some(Self::remove_callback);
        if let Some(t) = T::OF_DEVICE_ID_TABLE {
            pdrv.driver.of_match_table = t.as_ref();
        }
        if let Some(t) = T::ID_TABLE {
            pdrv.id_table = t.as_ref();
        }

        // SAFETY:
        //   - `pdrv` lives at least until the call to `platform_driver_unregister()` returns.
        //   - `name` pointer has static lifetime.
        //   - `module.0` lives at least as long as the module.
        //   - `probe()` and `remove()` are static functions.
        //   - `of_match_table` is either a raw pointer with static lifetime,
        //      as guaranteed by the [`driver::IdTable`] type, or null.
        //   - `id_table` is either a raw pointer with static lifetime,
        //      as guaranteed by the [`driver::IdTable`] type, or null.
        to_result(unsafe { bindings::i2c_register_driver(module.0, reg) })
    }

    unsafe fn unregister(reg: *mut bindings::i2c_driver) {
        // SAFETY: By the safety requirements of this function (defined in the trait definition),
        // `reg` was passed (and updated) by a previous successful call to
        // `i2c_register_driver`.
        unsafe { bindings::i2c_del_driver(reg) };
    }
}

impl<T: Driver> DriverAdapter<T> {
    fn get_id_info(client: &Client) -> Option<&'static T::IdInfo> {
        let table = T::OF_DEVICE_ID_TABLE?;

        // SAFETY: `table` has static lifetime, so it is valid for read. `client` is guaranteed to be
        // valid while it's alive, so is the raw device returned by it.
        let id = unsafe { bindings::of_match_device(table.as_ref(), client.raw_device()) };
        if id.is_null() {
            return None;
        }

        // SAFETY: `id` is a pointer within the static table, so it's always valid.
        let offset = unsafe { (*id).data };
        if offset.is_null() {
            return None;
        }

        // SAFETY: The offset comes from a previous call to `offset_from` in `IdArray::new`, which
        // guarantees that the resulting pointer is within the table.
        let ptr = unsafe {
            id.cast::<u8>()
                .offset(offset as _)
                .cast::<Option<T::IdInfo>>()
        };

        // SAFETY: The id table has a static lifetime, so `ptr` is guaranteed to be valid for read.
        unsafe { (&*ptr).as_ref() }
    }

    fn get_device_id_info(client: &Client) -> Option<&'static T::DeviceIdInfo> {
        let table = T::ID_TABLE?;

        // SAFETY: `table` has static lifetime, so it is valid for read. `client` is guaranteed to be
        // valid while it's alive, so is the raw device returned by it.
        let id = unsafe { bindings::i2c_match_id(table.as_ref(), client.raw_i2c_client()) };
        if id.is_null() {
            return None;
        }

        // SAFETY: `id` is a pointer within the static table, so it's always valid.
        let offset = unsafe { (*id).driver_data as *const c_void };
        if offset.is_null() {
            return None;
        }

        // SAFETY: The offset comes from a previous call to `offset_from` in `IdArray::new`, which
        // guarantees that the resulting pointer is within the table.
        let ptr = unsafe {
            id.cast::<u8>()
                .offset(offset as _)
                .cast::<Option<T::DeviceIdInfo>>()
        };

        // SAFETY: The id table has a static lifetime, so `ptr` is guaranteed to be valid for read.
        unsafe { (&*ptr).as_ref() }
    }

    extern "C" fn probe_new_callback(pclient: *mut bindings::i2c_client) -> core::ffi::c_int {
        from_kernel_result! {
            // SAFETY: `pclient` is valid by the contract with the C code. `dev` is alive only for the
            // duration of this call, so it is guaranteed to remain alive for the lifetime of
            // `pdev`.
            let mut client = unsafe { Client::from_ptr(pclient) };
            let info = Self::get_id_info(&client);
            let device_info = Self::get_device_id_info(&client);
            let data = T::probe(&mut client, info, device_info)?;
            // SAFETY: `pclient` is guaranteed to be a valid, non-null pointer.
            unsafe { bindings::i2c_set_clientdata(pclient, data.into_pointer() as _) };
            Ok(0)
        }
    }

    extern "C" fn remove_callback(pclient: *mut bindings::i2c_client) -> core::ffi::c_int {
        from_kernel_result! {
            // SAFETY: `pclient` is guaranteed to be a valid, non-null pointer.
            let ptr = unsafe { bindings::i2c_get_clientdata(pclient) };
            // SAFETY:
            //   - we allocated this pointer using `T::Data::into_pointer`,
            //     so it is safe to turn back into a `T::Data`.
            //   - the allocation happened in `probe`, no-one freed the memory,
            //     `remove` is the canonical kernel location to free driver data. so OK
            //     to convert the pointer back to a Rust structure here.
            let data = unsafe { T::Data::from_pointer(ptr) };
            let ret = T::remove(&data);
            <T::Data as driver::DeviceRemoval>::device_remove(&data);
            ret?;
            Ok(0)
        }
    }
}

/// An I2C device id.
#[derive(Clone, Copy)]
pub enum DeviceId {
    /// An I2C device name.
    Name(&'static BStr),
}

/// Defines a const I2C device id table that also carries per-entry data/context/info.
///
/// The name of the const is `ID_TABLE`, which is what buses are expected to name their
/// device id tables.
///
/// # Examples
///
/// ```
/// # use kernel::define_i2c_id_table;
/// use kernel::i2c;
///
/// define_of_id_table! {u32, [
///     (i2c::DeviceId::Name(b"i2cdev1"), Some(0xff)),
///     (i2c::DeviceId::Name(b"i2cdev2"), None),
/// ]};
/// ```
#[macro_export]
macro_rules! define_i2c_id_table {
    ($data_type:ty, $($t:tt)*) => {
        $crate::define_id_table!(ID_TABLE, $crate::i2c::DeviceId, $data_type, $($t)*);
    };
}

// SAFETY: `ZERO` is all zeroed-out and `to_rawid` stores `offset` in `i2c_device_id::driver_data`.
unsafe impl const driver::RawDeviceId for DeviceId {
    type RawType = bindings::i2c_device_id;
    const ZERO: Self::RawType = bindings::i2c_device_id {
        name: [0; 20],
        driver_data: 0,
    };

    fn to_rawid(&self, offset: isize) -> Self::RawType {
        let DeviceId::Name(name) = self;
        let mut id = Self::ZERO;
        let mut i = 0;
        while i < name.len() {
            // If `name` does not fit in `id.name`, an "index out of bounds" build time
            // error will be triggered.
            id.name[i] = name[i] as _;
            i += 1;
        }
        id.name[i] = b'\0' as _;
        id.driver_data = offset as _;
        id
    }
}

/// Represents an I2C device driver.
pub trait Driver {
    /// Data stored on device by driver.
    ///
    /// Corresponds to the data set or retrieved via the kernel's
    /// `i2c{set,get}_drvdata()` functions.
    ///
    /// Require that `Data` implements `PointerWrapper`. We guarantee to
    /// never move the underlying wrapped data structure. This allows
    type Data: PointerWrapper + Send + Sync + driver::DeviceRemoval = ();

    /// The type holding information about each device id supported by the driver.
    type IdInfo: 'static = ();

    /// The table of device ids supported by the driver.
    const OF_DEVICE_ID_TABLE: Option<driver::IdTable<'static, of::DeviceId, Self::IdInfo>> = None;

    /// The type holding information about each I2C device id supported by the driver.
    type DeviceIdInfo: 'static = ();

    /// List of I2C devices supported by this driver.
    const ID_TABLE: Option<driver::IdTable<'static, DeviceId, Self::DeviceIdInfo>> = None;

    /// Device binding.
    ///
    /// Called when a new platform device is added or discovered.
    /// Implementers should attempt to initialize the device here.
    fn probe(
        client: &mut Client,
        id_info: Option<&Self::IdInfo>,
        device_id_info: Option<&Self::DeviceIdInfo>,
    ) -> Result<Self::Data>;

    /// Device unbinding.
    ///
    /// Called when a platform device is removed.
    /// Implementers should prepare the device for complete removal here.
    fn remove(_data: &Self::Data) -> Result {
        Ok(())
    }
}
//
/// Represents an I2C slave device.
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Client {
    ptr: *mut bindings::i2c_client,
}

impl Client {
    /// Creates a new client from the given pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid. It must remain valid for the lifetime of the returned
    /// instance.
    unsafe fn from_ptr(ptr: *mut bindings::i2c_client) -> Self {
        // INVARIANT: The safety requirements of the function ensure the lifetime invariant.
        Self { ptr }
    }

    /// Returns the raw `struct i2c_client` related to `self`.
    unsafe fn raw_i2c_client(&self) -> *mut bindings::i2c_client {
        self.ptr
    }

    /// Address used on the I2C bus connected to the parent adapter.
    ///
    /// 7-bit addresses are stored in the lower 7 bits.
    pub fn addr(&self) -> u16 {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { (*self.ptr).addr }
    }
}

// SAFETY: The device returned by `raw_device` is the raw platform device.
unsafe impl device::RawDevice for Client {
    fn raw_device(&self) -> *mut bindings::device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { &mut (*self.ptr).dev }
    }
}

/// Declares a kernel module that exposes a single I2C device driver.
///
/// The `type` argument should be a type which implements the [`Driver`] trait. Also accepts
/// various forms of kernel metadata.
///
/// C header: [`include/linux/moduleparam.h`](../../../include/linux/moduleparam.h)
///
/// # Examples
///
/// ```ignore
/// use kernel::prelude::*;
///
/// struct MyDriver;
/// impl i2c::Driver for MyDriver {
///     // [...]
/// #   fn probe(_client: &mut Client,
/// #            _id_info: Option<&Self::IdInfo>,
/// #            _device_id_info: Option<&Self::DeviceIdInfo>) -> Result
/// #   {
/// #       Ok(())
/// #   }
/// }
///
/// module_i2c_driver! {
///     type: MyDriver,
///     name: b"my_i2cdev_kernel_module",
///     author: b"Author name",
///     description: b"My very own I2C device driver!",
///     license: b"GPL",
/// }
/// ```
#[macro_export]
macro_rules! module_i2c_driver {
    ($($f:tt)*) => {
        $crate::module_driver!(<T>, $crate::i2c::DriverAdapter<T>, { $($f)* });
    };
}
