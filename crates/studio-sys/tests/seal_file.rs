// SPDX-License-Identifier: GPL-3.0-or-later

//! Seals one raw image file with the C sealer, so a script can compare the
//! result with what `rimg.py seal` gives for the same file. It does nothing
//! unless `ROBCO_TEST_SEAL_RAW` and `ROBCO_TEST_SEAL_OUT` are set:
//!
//! - `ROBCO_TEST_SEAL_RAW`: the raw image, as `objcopy -O binary` writes it.
//! - `ROBCO_TEST_SEAL_OUT`: where the sealed file goes.
//! - `ROBCO_TEST_SEAL_NAME`, `ROBCO_TEST_SEAL_VERSION` (X.Y.Z),
//!   `ROBCO_TEST_SEAL_TITLE` and `ROBCO_TEST_SEAL_ICON` (a file, read with
//!   the auto rule): each one optional, as the options of `rimg.py seal`.
//!
//! `tools/agreement.sh` runs it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file; a failed unwrap is a failed test"
)]

use std::fs;

use studio_sys::{Icon, IconFormat, SealOpts, rimg_seal};

#[test]
fn seal_the_file_the_environment_names() {
    let (Some(raw), Some(out)) = (
        std::env::var_os("ROBCO_TEST_SEAL_RAW"),
        std::env::var_os("ROBCO_TEST_SEAL_OUT"),
    ) else {
        return;
    };
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let name = var("ROBCO_TEST_SEAL_NAME");
    let title = var("ROBCO_TEST_SEAL_TITLE");
    let version = var("ROBCO_TEST_SEAL_VERSION").map(|v| {
        let n: Vec<u32> = v.split('.').map(|p| p.parse().unwrap()).collect();
        assert_eq!(n.len(), 3, "ROBCO_TEST_SEAL_VERSION {v} is not X.Y.Z");
        (n[0] << 16) | (n[1] << 8) | n[2]
    });
    let icon = var("ROBCO_TEST_SEAL_ICON").map(|p| fs::read(p).unwrap());
    let opts = SealOpts {
        name: name.as_deref(),
        image_version: version,
        title: title.as_deref(),
        icon: icon.as_deref().map(|bytes| Icon {
            bytes,
            format: IconFormat::Auto,
        }),
    };
    let sealed = rimg_seal(&fs::read(raw).unwrap(), &opts).expect("the C sealer seals the file");
    fs::write(out, sealed).unwrap();
}
