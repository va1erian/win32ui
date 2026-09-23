//! DirectWrite text: metrics, family resolution, threading, the width cache
//! and hit testing. None of this needs a window.

#![cfg(windows)]

use win32ui::d2d::{FontSpec, FontStretch, TextSystem};

fn system() -> TextSystem {
    TextSystem::new().expect("DirectWrite is available")
}

#[test]
fn metrics_are_sane() {
    let font = system()
        .font(&FontSpec::new("Segoe UI", 16.0))
        .expect("font");
    let metrics = font.metrics();
    assert!(metrics.ascent + metrics.descent > 0.0);
    assert!(metrics.cap_height > metrics.x_height && metrics.x_height > 0.0);
    assert!(metrics.line_height() >= metrics.ascent + metrics.descent);

    assert_eq!(font.width(""), 0.0);
    let short = font.width("Hello");
    let long = font.width("Hello, world");
    assert!(short > 0.0 && long > short, "{short} vs {long}");
}

#[test]
fn bigger_sizes_and_bolder_weights_are_wider() {
    let system = system();
    let text = "The quick brown fox";
    let regular = system.font(&FontSpec::new("Segoe UI", 16.0)).unwrap();
    let large = system.font(&FontSpec::new("Segoe UI", 32.0)).unwrap();
    let bold = system
        .font(&FontSpec::new("Segoe UI", 16.0).weight(700))
        .unwrap();
    assert!((large.width(text) / regular.width(text) - 2.0).abs() < 0.1);
    assert!(bold.width(text) > regular.width(text));
}

#[test]
fn family_lists_resolve_entry_by_entry() {
    let system = system();
    let family = |list: &str| {
        system
            .font(&FontSpec::new(list, 14.0))
            .unwrap()
            .family()
            .to_owned()
    };
    assert_eq!(family("No Such Font 123, Arial"), "Arial");
    assert_eq!(family("'Segoe UI Semibold', Arial"), "Segoe UI Semibold");
    assert_eq!(family("Arial Black"), "Arial Black");
    assert_eq!(family("serif"), "Cambria");
    assert_eq!(family("sans-serif"), "Segoe UI");
    assert_eq!(family("monospace"), "Consolas");
    assert_eq!(
        family("No Such Font 123"),
        "Segoe UI",
        "falls back to the UI font"
    );
}

#[test]
fn weights_and_stretch_select_different_faces() {
    let system = system();
    let text = "Semibold text";
    let width = |weight| {
        system
            .font(&FontSpec::new("Segoe UI", 20.0).weight(weight))
            .unwrap()
            .width(text)
    };
    assert!(width(300) < width(400));
    assert!(width(400) < width(600));
    assert!(width(600) < width(700));
    let narrow = system
        .font(&FontSpec::new("Arial", 20.0).stretch(FontStretch::Condensed))
        .unwrap();
    assert!(narrow.width(text) > 0.0);
}

#[test]
fn italic_is_available() {
    let system = system();
    let italic = system
        .font(&FontSpec::new("Segoe UI", 16.0).italic(true))
        .unwrap();
    assert!(italic.width("italic") > 0.0);
}

#[test]
fn invalid_sizes_are_rejected() {
    let system = system();
    for size in [0.0, -4.0, f32::NAN, f32::INFINITY] {
        assert!(system.font(&FontSpec::new("Segoe UI", size)).is_err());
    }
}

#[test]
fn the_same_spec_gives_the_same_font() {
    let system = system();
    let spec = FontSpec::new("Segoe UI", 13.0);
    let first = system.font(&spec).unwrap();
    first.width("cached");
    assert_eq!(system.font(&spec).unwrap().cached_widths(), 1);
}

#[test]
fn measurement_works_from_a_worker_thread() {
    let system = system();
    let font = system.font(&FontSpec::new("Segoe UI", 16.0)).unwrap();
    let expected = font.width("Hello 👋 世界");
    let worker = std::thread::spawn({
        let system = system.clone();
        let font = font.clone();
        move || {
            let other = system.font(&FontSpec::new("Arial", 12.0)).unwrap();
            let layout = font.layout("worker layout", 100.0).unwrap();
            (
                font.width("Hello 👋 世界"),
                other.width("abc"),
                layout.width(),
            )
        }
    });
    let (width, other, layout) = worker.join().expect("worker panicked");
    assert_eq!(width, expected);
    assert!(other > 0.0 && layout > 0.0);
}

#[test]
fn many_threads_measure_concurrently() {
    let font = system().font(&FontSpec::new("Segoe UI", 16.0)).unwrap();
    let expected: Vec<f32> = (0..50).map(|n| font.width(&format!("word {n}"))).collect();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let font = font.clone();
            std::thread::spawn(move || {
                (0..50)
                    .map(|n| font.width(&format!("word {n}")))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().unwrap(), expected);
    }
}

#[test]
fn width_is_cached_and_bounded() {
    let font = system().font(&FontSpec::new("Segoe UI", 15.0)).unwrap();
    assert_eq!(font.cached_widths(), 0);
    let first = font.width("repeat");
    font.width("repeat");
    font.width("again");
    assert_eq!(font.cached_widths(), 2);
    assert_eq!(font.width("repeat"), first);

    for n in 0..20_000 {
        font.width(&format!("string {n}"));
    }
    assert!(font.cached_widths() <= 16_384, "{}", font.cached_widths());

    let huge = "long ".repeat(200);
    let before = font.cached_widths();
    assert!(font.width(&huge) > 0.0);
    assert_eq!(font.cached_widths(), before, "long strings are not cached");
}

#[test]
fn fallback_measures_like_it_lays_out() {
    let font = system().font(&FontSpec::new("Segoe UI", 16.0)).unwrap();
    for text in ["世界", "مرحبا", "👋", "Hello 👋 世界"] {
        let measured = font.width(text);
        let laid_out = font.layout(text, f32::INFINITY).unwrap().width();
        assert!(measured > 0.0, "{text}");
        assert_eq!(measured, laid_out, "{text}");
    }
    assert!(
        font.width("世界") > font.width("ab"),
        "CJK is not zero-width or tofu-narrow"
    );
}

#[test]
fn a_list_falls_back_to_a_later_family_for_missing_glyphs() {
    let system = system();
    let text = "abc 世界";
    let listed = system
        .font(&FontSpec::new("Consolas, Segoe UI", 16.0))
        .unwrap();
    assert_eq!(listed.family(), "Consolas");
    assert!(listed.width(text) > 0.0);
}

#[test]
fn hit_testing_round_trips_through_carets() {
    let font = system().font(&FontSpec::new("Segoe UI", 18.0)).unwrap();
    let text = "Round trip é世👋z";
    let layout = font.layout(text, f32::INFINITY).unwrap();
    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()]);
    for index in boundaries {
        let caret = layout.caret_rect(index);
        assert!(caret.height() > 0.0);
        let hit = layout.hit_test_point(caret.left + 0.1, (caret.top + caret.bottom) / 2.0);
        assert_eq!(hit.index, index, "caret at byte {index}");
    }
    let past = layout.hit_test_point(layout.width() + 50.0, 5.0);
    assert!(!past.inside);
    assert_eq!(past.index, text.len());
}

#[test]
fn selections_and_lines_follow_wrapping() {
    let font = system().font(&FontSpec::new("Segoe UI", 16.0)).unwrap();
    let text = "wrap these words onto several lines please";
    let layout = font.layout(text, 90.0).unwrap();
    let lines = layout.lines();
    assert!(lines.len() > 2, "{} lines", lines.len());
    assert_eq!(lines.first().unwrap().range.start, 0);
    assert_eq!(lines.last().unwrap().range.end, text.len());
    assert!(
        lines
            .windows(2)
            .all(|pair| pair[0].range.end == pair[1].range.start)
    );
    assert!(layout.height() >= lines.iter().map(|line| line.height).sum::<f32>() - 0.01);

    assert!(layout.selection_rects(3, 3).is_empty());
    let selection = layout.selection_rects(0, text.len());
    assert_eq!(selection.len(), lines.len(), "one box per line");
    let one_word = layout.selection_rects(0, 4);
    assert_eq!(one_word.len(), 1);
    assert!((one_word[0].width() - font.width("wrap")).abs() < 0.5);
}

#[test]
fn an_empty_layout_is_harmless() {
    let font = system().font(&FontSpec::new("Segoe UI", 16.0)).unwrap();
    let layout = font.layout("", 100.0).unwrap();
    assert_eq!(layout.width(), 0.0);
    assert!(layout.height() > 0.0, "an empty line still has a height");
    assert_eq!(layout.hit_test_point(3.0, 3.0).index, 0);
    assert!(layout.selection_rects(0, 5).is_empty());
}

#[test]
fn a_legacy_gdi_name_selects_its_own_face() {
    let system = system();
    let width = |family: &str| {
        system
            .font(&FontSpec::new(family, 20.0))
            .unwrap()
            .width("Semibold face")
    };
    assert!(width("Segoe UI Semibold") > width("Segoe UI"));
}

/// `cargo test --release --test text -- --ignored --nocapture measure_10k`
#[test]
#[ignore = "a benchmark, run on demand"]
fn measure_10k_short_strings() {
    let font = system().font(&FontSpec::new("Segoe UI", 16.0)).unwrap();
    let words: Vec<String> = (0..10_000).map(|n| format!("word{n} héllo")).collect();
    let time = |label: &str| {
        let start = std::time::Instant::now();
        let total: f32 = words.iter().map(|word| font.width(word)).sum();
        println!(
            "{label}: {:?} for {} strings (sum {total})",
            start.elapsed(),
            words.len()
        );
    };
    time("cold (every string measured by DirectWrite)");
    time("warm (every string cached)");
}
