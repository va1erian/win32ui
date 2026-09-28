use windows::Win32::System::Com::{FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};

use super::*;

#[test]
fn payload_round_trips_through_the_data_object() {
    let data = DataObj::with_payload(b"tracks:1,2,3");
    assert!(data.has_payload());
    assert_eq!(data.payload().as_deref(), Some(&b"tracks:1,2,3"[..]));
    assert!(!data.has_hdrop());
    assert_eq!(data.hdrop(), None);
}

#[test]
fn empty_payload_is_still_present() {
    let data = DataObj::with_payload(&[]);
    assert!(data.has_payload());
    assert_eq!(data.payload(), Some(Vec::new()));
}

#[test]
fn set_data_stores_a_private_format_and_releases_the_medium() {
    let data = DataObj::with_payload(b"x");
    let format = format_etc(0xC123);
    let medium = STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 {
            hGlobal: hglobal::alloc(b"helper bits").expect("alloc"),
        },
        pUnkForRelease: ManuallyDrop::new(None),
    };
    // SAFETY: valid pointers for the call; `fRelease` hands the medium over.
    unsafe {
        data.com()
            .SetData(&format as *const FORMATETC, &medium, true)
    }
    .expect("SetData accepts an HGLOBAL");
    assert_eq!(data.read(0xC123).as_deref(), Some(&b"helper bits"[..]));
    assert!(data.has_payload(), "the payload survives a SetData");
}

#[test]
fn unknown_formats_are_refused() {
    let data = DataObj::with_payload(b"x");
    assert!(!data.has(0xC999));
    assert_eq!(data.read(0xC999), None);
}
