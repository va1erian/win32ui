//! Screenshot any top-level window by handle, title substring or process id.
//!
//! ```text
//! cargo run --features wgc --example capture -- --hwnd 0x00040A2C --out shot.png
//! cargo run --features wgc --example capture -- --title "My App" --out shot.png
//! cargo run --features wgc --example capture -- --pid 1234 --out shot.png
//! ```
//!
//! Uses `Windows.Graphics.Capture` (`win32ui::capture::capture_hwnd`), so the
//! window does not have to be visible, unoccluded or focused, and the tool
//! never raises it or moves the pointer. This is what agents and test harnesses
//! should use instead of a screen capture. Window lookup by title/pid is only
//! performed when the matching flag is passed.

#[cfg(feature = "wgc")]
mod tool {
    use std::fs::File;
    use std::io::BufWriter;
    use std::path::PathBuf;

    use win32ui::prelude::*;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    };
    use windows::core::BOOL;

    /// The selector the user gave; exactly one is required.
    enum Selector {
        Hwnd(Hwnd),
        Title(String),
        Pid(u32),
    }

    /// What the callback is searching for, plus the handles it has found.
    struct Search {
        selector: Selector,
        matches: Vec<usize>,
    }

    struct Options {
        selector: Selector,
        out: PathBuf,
    }

    pub fn run() {
        let Some(options) = parse_args() else {
            print_usage();
            std::process::exit(2);
        };
        let Some(hwnd) = resolve(options.selector) else {
            eprintln!("capture: no visible top-level window matched");
            std::process::exit(1);
        };
        match win32ui::capture::capture_hwnd(hwnd) {
            Ok(image) => {
                if let Err(error) = write_png(&image, &options.out) {
                    eprintln!("capture: writing {} failed: {error}", options.out.display());
                    std::process::exit(1);
                }
                eprintln!(
                    "capture: wrote {} ({}x{}) from hwnd 0x{:x}",
                    options.out.display(),
                    image.width,
                    image.height,
                    hwnd.raw()
                );
            }
            Err(error) => {
                eprintln!("capture: {error}");
                std::process::exit(1);
            }
        }
    }

    fn parse_args() -> Option<Options> {
        let mut selector = None;
        let mut out = None;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--hwnd" => {
                    let value = args.next()?;
                    let raw = if let Some(hex) = value.strip_prefix("0x") {
                        usize::from_str_radix(hex, 16).ok()?
                    } else {
                        value.parse().ok()?
                    };
                    selector = Some(Selector::Hwnd(Hwnd::from_raw(raw)));
                }
                "--title" => selector = Some(Selector::Title(args.next()?)),
                "--pid" => selector = Some(Selector::Pid(args.next()?.parse().ok()?)),
                "--out" => out = Some(PathBuf::from(args.next()?)),
                "-h" | "--help" => return None,
                _ => return None,
            }
        }
        Some(Options {
            selector: selector?,
            out: out?,
        })
    }

    fn print_usage() {
        eprintln!(
            "usage: capture (--hwnd <n|0xhex> | --title <substring> | --pid <n>) --out <file.png>"
        );
    }

    /// Resolves the selector to a live window handle, enumerating visible
    /// top-level windows only for the opt-in `--title`/`--pid` flags.
    fn resolve(selector: Selector) -> Option<Hwnd> {
        match selector {
            Selector::Hwnd(hwnd) => hwnd.is_alive().then_some(hwnd),
            selector => {
                let mut search = Search {
                    selector,
                    matches: Vec::new(),
                };
                // SAFETY: `search` outlives the synchronous enumeration and the
                // callback only writes to it through the same pointer.
                unsafe {
                    EnumWindows(
                        Some(enum_proc),
                        LPARAM((&mut search as *mut Search).cast::<core::ffi::c_void>() as isize),
                    )
                }
                .ok()?;
                search.matches.first().map(|&raw| Hwnd::from_raw(raw))
            }
        }
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `resolve` passes a pointer to a live `Search` that outlives
        // the enumeration; `EnumWindows` never runs the callback off-thread.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return BOOL(1);
        }
        match &search.selector {
            Selector::Pid(pid) => {
                let mut window_pid = 0u32;
                // SAFETY: plain out-pointer to a local.
                unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
                if window_pid == *pid {
                    search.matches.push(hwnd.0 as usize);
                }
            }
            Selector::Title(needle) => {
                // SAFETY: `hwnd` is live for the duration of the callback.
                let len = unsafe { GetWindowTextLengthW(hwnd) };
                if len > 0 {
                    let mut buf = vec![0u16; len as usize + 1];
                    // SAFETY: `buf` has room for `len` code units plus the NUL.
                    let written = unsafe { GetWindowTextW(hwnd, &mut buf) };
                    let text = String::from_utf16_lossy(&buf[..written as usize]);
                    if text.contains(needle.as_str()) {
                        search.matches.push(hwnd.0 as usize);
                    }
                }
            }
            Selector::Hwnd(_) => {}
        }
        BOOL(1)
    }

    fn write_png(image: &RgbaImage, path: &std::path::Path) -> std::io::Result<()> {
        let file = File::create(path)?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        writer
            .write_image_data(&image.pixels)
            .map_err(|error| std::io::Error::other(error.to_string()))
    }
}

#[cfg(feature = "wgc")]
fn main() {
    tool::run()
}

#[cfg(not(feature = "wgc"))]
fn main() {
    eprintln!("capture: rebuild with `--features wgc` to enable composited window capture");
    std::process::exit(2);
}
