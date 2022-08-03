// SPDX-License-Identifier: GPL-2.0

//! MLI-Labs Embedded Management Controller (EMC) driver.

use kernel::{module_platform_driver, of, platform, prelude::*};

module_platform_driver! {
    type: Driver,
    name: b"mlilabs_emc",
    license: b"GPL",
}

struct Driver;
impl platform::Driver for Driver {
    kernel::define_of_id_table! {(), [
        (of::DeviceId::Compatible(b"mlilabs,emc"), None),
    ]}

    fn probe(_dev: &mut platform::Device, _id_info: Option<&Self::IdInfo>) -> Result {
        Ok(())
    }
}
