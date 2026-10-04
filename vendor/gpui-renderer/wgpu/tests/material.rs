//! Shader and record-layout checks do not require a platform window or font fixtures.
use gpui::BackdropBlur;

const STORAGE: &str = concat!(
    include_str!("../src/shaders.wgsl"),
    include_str!("../src/shaders_storage.wgsl")
);
const WEBGL: &str = concat!(
    include_str!("../src/shaders.wgsl"),
    include_str!("../src/shaders_webgl.wgsl")
);
const SUBPIXEL: &str = concat!(
    "enable dual_source_blending;\n",
    include_str!("../src/shaders.wgsl"),
    include_str!("../src/shaders_storage.wgsl"),
    include_str!("../src/shaders_subpixel.wgsl"),
);

fn validate(source: &str, capabilities: naga::valid::Capabilities) {
    let module = naga::front::wgsl::parse_str(source).expect("shader should parse");
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
        .validate(&module)
        .expect("shader should validate");
}

#[test]
fn storage_shader() {
    validate(STORAGE, naga::valid::Capabilities::empty());
}

#[test]
fn webgl_shader() {
    assert!(!WEBGL.contains("var<storage"));
    validate(WEBGL, naga::valid::Capabilities::empty());
}

#[test]
fn subpixel_shader() {
    validate(SUBPIXEL, naga::valid::Capabilities::DUAL_SOURCE_BLENDING);
}

#[test]
fn backdrop_record_stride() {
    assert_eq!(std::mem::size_of::<BackdropBlur>(), 28 * 4);
}
