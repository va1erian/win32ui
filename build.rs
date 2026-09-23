//! Embeds `win32ui.rc` (the application manifest, which turns on Common
//! Controls v6 and per-monitor-v2 DPI) into the example binaries. Resources
//! only make sense for an executable, so the library itself and its tests are
//! left untouched; on non-Windows hosts this is a no-op.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=win32ui.rc");
    println!("cargo:rerun-if-changed=win32ui.manifest");

    embed_resource::compile_for_examples("win32ui.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compile the win32ui example manifest");
}
