//! The WGSL parses and validates with naga, the same front end wgpu uses. No GPU.

#[test]
fn wgsl_validates() {
    let module = naga::front::wgsl::parse_str(k8_crt::WGSL).expect("parse");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("validate");
    for e in [
        "vs",
        "fs_bright",
        "fs_blur_h",
        "fs_blur_v",
        "fs_comp",
        "fs_blit",
    ] {
        assert!(module.entry_points.iter().any(|p| p.name == e), "{e}");
    }
}
