//! The drop target driven directly through its COM interface, with no window
//! and no pointer: enter, over and drop reach the sink, in order, with the
//! payload, and the effect the sink picks is masked by what the source allows.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;

use super::*;

/// Records every call and answers with a fixed effect.
struct Recorder {
    answer: u32,
    calls: RefCell<Vec<String>>,
}

impl TargetSink for Recorder {
    fn enter(&self, point: DragPoint, data: &DataObj) -> u32 {
        self.calls.borrow_mut().push(format!(
            "enter {:?} allowed={} payload={:?}",
            (point.x, point.y),
            point.allowed,
            data.payload()
        ));
        self.answer
    }

    fn over(&self, point: DragPoint, _data: &DataObj) -> u32 {
        self.calls
            .borrow_mut()
            .push(format!("over {:?}", (point.x, point.y)));
        self.answer
    }

    fn leave(&self) {
        self.calls.borrow_mut().push("leave".to_string());
    }

    fn dropped(&self, point: DragPoint, data: &DataObj) -> u32 {
        self.calls.borrow_mut().push(format!(
            "drop {:?} payload={:?}",
            (point.x, point.y),
            data.payload()
        ));
        self.answer
    }
}

fn recorder(answer: u32) -> Rc<Recorder> {
    Rc::new(Recorder {
        answer,
        calls: RefCell::new(Vec::new()),
    })
}

/// A target with no window: `ScreenToClient` on a null handle leaves the
/// point unchanged, so screen and client coordinates coincide.
///
/// It has no shell drag-image helper. The helper's drag image is shared by
/// the whole process, and these tests run on parallel threads with no window
/// behind the target; driving it that way crashed the test binary
/// intermittently with an access violation. The sink is what is under test.
fn target_for(sink: &Rc<Recorder>) -> IDropTarget {
    ole::ensure().expect("OLE initialises on the test thread");
    let sink: Rc<dyn TargetSink> = sink.clone();
    make_target(HWND(std::ptr::null_mut()), sink, None)
}

#[test]
fn enter_over_and_drop_reach_the_sink_with_the_payload() {
    let sink = recorder(DROPEFFECT_MOVE.0);
    let target = target_for(&sink);
    let data = DataObj::with_payload(b"abc");
    let mut effect = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0);
    let at = POINTL { x: 11, y: 22 };
    let keys = MODIFIERKEYS_FLAGS(0);

    // SAFETY: valid pointers for each call; the data object outlives them.
    unsafe {
        target
            .DragEnter(data.com(), keys, at, &mut effect)
            .expect("enter");
        assert_eq!(effect, DROPEFFECT_MOVE);
        effect = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0);
        target.DragOver(keys, at, &mut effect).expect("over");
        assert_eq!(effect, DROPEFFECT_MOVE);
        effect = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0);
        target
            .Drop(data.com(), keys, at, &mut effect)
            .expect("drop");
        assert_eq!(effect, DROPEFFECT_MOVE);
    }

    let calls = sink.calls.borrow();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(
        calls[0].starts_with("enter (11, 22) allowed=3"),
        "{calls:?}"
    );
    assert!(calls[0].contains(r#"Some([97, 98, 99])"#), "{calls:?}");
    assert_eq!(calls[1], "over (11, 22)");
    assert!(calls[2].starts_with("drop (11, 22)"), "{calls:?}");
}

#[test]
fn the_sink_cannot_grant_an_effect_the_source_forbids() {
    let sink = recorder(DROPEFFECT_MOVE.0);
    let target = target_for(&sink);
    let data = DataObj::with_payload(b"x");
    let mut effect = DROPEFFECT_COPY;
    // SAFETY: valid pointers for the call.
    unsafe {
        target
            .DragEnter(
                data.com(),
                MODIFIERKEYS_FLAGS(0),
                POINTL { x: 0, y: 0 },
                &mut effect,
            )
            .expect("enter");
    }
    assert_eq!(
        effect.0, 0,
        "only copy was allowed, the sink asked for move"
    );
}

#[test]
fn leave_reaches_the_sink_and_stops_over_events() {
    let sink = recorder(DROPEFFECT_COPY.0);
    let target = target_for(&sink);
    let data = DataObj::with_payload(b"x");
    let mut effect = DROPEFFECT_COPY;
    let keys = MODIFIERKEYS_FLAGS(0);
    let at = POINTL { x: 1, y: 2 };
    // SAFETY: valid pointers for each call.
    unsafe {
        target
            .DragEnter(data.com(), keys, at, &mut effect)
            .expect("enter");
        target.DragLeave().expect("leave");
        effect = DROPEFFECT_COPY;
        target.DragOver(keys, at, &mut effect).expect("stray over");
    }
    assert_eq!(effect.0, 0, "an over after leave is rejected");
    let calls = sink.calls.borrow();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[1], "leave");
}
