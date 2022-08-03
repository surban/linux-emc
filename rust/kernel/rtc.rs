// SPDX-License-Identifier: GPL-2.0

//! Real time clock
//!
//! C header: [`include/linux/rtc.h`](../../../../include/linux/rtc.h)

use crate::{
    bindings, device,
    error::{from_kernel_err_ptr, from_kernel_result},
    prelude::*,
    types::PointerWrapper,
};
use core::{cell::UnsafeCell, marker::PhantomData};

/// RTC time.
pub struct RtcTime {
    ptr: *mut bindings::rtc_time,
}

impl RtcTime {
    /// Creates a new RTC time from the given pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid. It must remain valid for the lifetime of the returned
    /// instance.
    unsafe fn from_ptr(ptr: *mut bindings::rtc_time) -> Self {
        Self { ptr }
    }

    /// Seconds.
    pub fn sec(&self) -> i32 {
        unsafe { (*self.ptr).tm_sec }
    }

    /// Sets the seconds.
    pub fn set_sec(&mut self, sec: i32) {
        unsafe { (*self.ptr).tm_sec = sec }
    }

    /// Minutes.
    pub fn min(&self) -> i32 {
        unsafe { (*self.ptr).tm_min }
    }

    /// Sets the minutes.
    pub fn set_min(&mut self, min: i32) {
        unsafe { (*self.ptr).tm_min = min }
    }

    /// Hours.
    pub fn hour(&self) -> i32 {
        unsafe { (*self.ptr).tm_hour }
    }

    /// Sets the hours.
    pub fn set_hour(&mut self, hour: i32) {
        unsafe { (*self.ptr).tm_hour = hour }
    }

    /// Day of month.
    pub fn mday(&self) -> i32 {
        unsafe { (*self.ptr).tm_mday }
    }

    /// Sets the day of month.
    pub fn set_mday(&mut self, mday: i32) {
        unsafe { (*self.ptr).tm_mday = mday }
    }

    /// Month.
    pub fn mon(&self) -> i32 {
        unsafe { (*self.ptr).tm_mon }
    }

    /// Sets the month.
    pub fn set_mon(&mut self, mon: i32) {
        unsafe { (*self.ptr).tm_mon = mon }
    }

    /// Year.
    pub fn year(&self) -> i32 {
        unsafe { (*self.ptr).tm_year }
    }

    /// Sets the year.
    pub fn set_year(&mut self, year: i32) {
        unsafe { (*self.ptr).tm_year = year }
    }

    /// Day of week.
    pub fn wday(&self) -> i32 {
        unsafe { (*self.ptr).tm_wday }
    }

    /// Sets the day of week.
    pub fn set_wday(&mut self, wday: i32) {
        unsafe { (*self.ptr).tm_wday = wday }
    }

    /// Day of year.
    pub fn yday(&self) -> i32 {
        unsafe { (*self.ptr).tm_yday }
    }

    /// Sets the day of year.
    pub fn set_yday(&mut self, yday: i32) {
        unsafe { (*self.ptr).tm_yday = yday }
    }

    /// Daylight saving time.
    pub fn isdst(&self) -> i32 {
        unsafe { (*self.ptr).tm_isdst }
    }

    /// Sets the daylight saving time.
    pub fn set_isdst(&mut self, isdst: i32) {
        unsafe { (*self.ptr).tm_isdst = isdst }
    }
}

/// A real time clock (RTC).
#[vtable]
pub trait Rtc {
    /// Context data associated with the gpio chip.
    ///
    /// It determines the type of the context data passed to each of the methods of the trait.
    type Data: PointerWrapper + Sync + Send;

    /// Reads the date and time from the RTC.
    fn read_time(_data: <Self::Data as PointerWrapper>::Borrowed<'_>, time: &mut RtcTime)
        -> Result;

    /// Sets the date and time of the RTC.
    fn set_time(_data: <Self::Data as PointerWrapper>::Borrowed<'_>, time: &RtcTime) -> Result;
}

/// A registration of a real time clock (RTC).
pub struct Registration<T: Rtc> {
    rtc: Option<*mut bindings::rtc_device>,
    ops: UnsafeCell<bindings::rtc_class_ops>,
    parent: Option<device::Device>,
    _p: PhantomData<T>,
}

impl<T: Rtc> Registration<T> {
    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new() -> Result<Self> {
        Ok(Self {
            rtc: None,
            ops: UnsafeCell::new(bindings::rtc_class_ops::default()),
            parent: None,
            _p: PhantomData,
        })
    }

    /// Registers a real time clock (RTC) with the rest of the kernel.
    pub fn register(self: Pin<&mut Self>, parent: &dyn device::RawDevice, data: T::Data) -> Result {
        if self.parent.is_some() {
            // Already registered.
            return Err(EINVAL);
        }

        // SAFETY: We never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };

        {
            // Set up the callbacks.
            let ops = this.ops.get_mut();
            if T::HAS_READ_TIME {
                ops.read_time = Some(read_time_callback::<T>);
            }
            if T::HAS_SET_TIME {
                ops.set_time = Some(set_time_callback::<T>);
            }
        }

        let rtc = unsafe {
            from_kernel_err_ptr(bindings::devm_rtc_allocate_device(parent.raw_device()))
        }?;
        unsafe { (*rtc).ops = this.ops.get() };
        this.rtc = Some(rtc);

        let data_pointer = <T::Data as PointerWrapper>::into_pointer(data);
        unsafe { bindings::dev_set_drvdata(&mut (*rtc).dev, data_pointer as *mut _) };

        // SAFETY: `rtc` was initilised above, so it is valid.
        let ret = unsafe { bindings::devm_rtc_register_device(rtc) };
        if ret < 0 {
            // SAFETY: `data_pointer` was returned by `into_pointer` above.
            unsafe { T::Data::from_pointer(data_pointer) };
            return Err(Error::from_kernel_errno(ret));
        }

        this.parent = Some(device::Device::from_dev(parent));
        Ok(())
    }
}

unsafe extern "C" fn read_time_callback<T: Rtc>(
    dev: *mut bindings::device,
    time: *mut bindings::rtc_time,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The value stored as chip data was returned by `into_pointer` during registration.
        let data = unsafe { T::Data::borrow(bindings::dev_get_drvdata(dev)) };
        let mut time = unsafe { RtcTime::from_ptr(time) };
        T::read_time(data, &mut time)?;
        Ok(0)
    }
}

unsafe extern "C" fn set_time_callback<T: Rtc>(
    dev: *mut bindings::device,
    time: *mut bindings::rtc_time,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The value stored as chip data was returned by `into_pointer` during registration.
        let data = unsafe { T::Data::borrow(bindings::dev_get_drvdata(dev)) };
        let time = unsafe { RtcTime::from_ptr(time) };
        T::set_time(data, &time)?;
        Ok(0)
    }
}
