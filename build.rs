//! Embeds `win32ui.rc` (the application manifest, which turns on Common
//! Controls v6 and per-monitor-v2 DPI) into the example and test binaries.
//! The tests create real common controls, which need the v6 manifest just as
//! the examples do. Resources only make sense for an executable, so the library
//! itself is left untouched; on non-Windows hosts this is a no-op.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=win32ui.rc");
    println!("cargo:rerun-if-changed=win32ui.manifest");
    println!("cargo:rerun-if-changed=win32ui.ico");

    embed_resource::compile_for_examples("win32ui.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compile the win32ui example manifest");
    embed_resource::compile_for_tests("win32ui.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compile the win32ui test manifest");
}
