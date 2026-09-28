//! The in-process `IDataObject` a drag carries, and the read-side wrapper used
//! for any data object (ours, or Explorer's file list).

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::sync::OnceLock;

use windows::Win32::Foundation::{DV_E_FORMATETC, E_NOTIMPL, OLE_E_ADVISENOTSUPPORTED};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Ole::{CF_HDROP, ReleaseStgMedium};
use windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc;
use windows::core::{BOOL, HRESULT, Ref, implement, w};

use super::hglobal;

/// The private clipboard format that carries the app's opaque payload.
pub(super) fn payload_format() -> u16 {
    static FORMAT: OnceLock<u16> = OnceLock::new();
    *FORMAT.get_or_init(|| {
        // SAFETY: registers (or looks up) a process-wide clipboard format by
        // name; the literal is a valid nul-terminated wide string.
        unsafe { RegisterClipboardFormatW(w!("win32ui.dnd.payload")) as u16 }
    })
}

/// Bytes of the length header in front of the payload.
const LENGTH_PREFIX: usize = 8;

/// A `FORMATETC` asking for `format` as an `HGLOBAL`.
fn format_etc(format: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// A data object holding byte blobs in `HGLOBAL`s, keyed by clipboard format.
/// It also accepts `SetData`, which is how the drag-image helper attaches its
/// bitmap and window handle.
#[implement(IDataObject)]
struct DataObject {
    entries: RefCell<Vec<(u16, Vec<u8>)>>,
}

impl DataObject {
    fn find(&self, format: u16) -> Option<Vec<u8>> {
        self.entries
            .borrow()
            .iter()
            .find(|(candidate, _)| *candidate == format)
            .map(|(_, bytes)| bytes.clone())
    }

    /// The clipboard format `format` asks for, when this object can hand it
    /// out as an `HGLOBAL`.
    fn supports(&self, format: *const FORMATETC) -> Option<u16> {
        // SAFETY: COM passes a valid `FORMATETC` (or null, handled by `as_ref`).
        let format = unsafe { format.as_ref() }?;
        let wants_hglobal = format.tymed & TYMED_HGLOBAL.0 as u32 != 0;
        let wants_content = format.dwAspect == DVASPECT_CONTENT.0;
        (wants_hglobal && wants_content && self.find(format.cfFormat).is_some())
            .then_some(format.cfFormat)
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for DataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let format = self
            .supports(pformatetcin)
            .ok_or_else(|| windows::core::Error::from(DV_E_FORMATETC))?;
        let bytes = self.find(format).unwrap_or_default();
        let handle = hglobal::alloc(&bytes)?;
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: handle },
            pUnkForRelease: ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        if self.supports(pformatetc).is_some() {
            HRESULT(0)
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        E_NOTIMPL
    }

    fn SetData(
        &self,
        pformatetc: *const FORMATETC,
        pmedium: *const STGMEDIUM,
        frelease: BOOL,
    ) -> windows::core::Result<()> {
        // SAFETY: COM passes valid pointers for the duration of the call.
        let (Some(format), Some(medium)) =
            (unsafe { pformatetc.as_ref() }, unsafe { pmedium.as_ref() })
        else {
            return Err(E_NOTIMPL.into());
        };
        if medium.tymed != TYMED_HGLOBAL.0 as u32 {
            return Err(DV_E_FORMATETC.into());
        }
        // SAFETY: `tymed` says the union holds an `HGLOBAL`.
        let bytes = hglobal::read(unsafe { medium.u.hGlobal })
            .ok_or_else(|| windows::core::Error::from(DV_E_FORMATETC))?;
        {
            let mut entries = self.entries.borrow_mut();
            entries.retain(|(candidate, _)| *candidate != format.cfFormat);
            entries.push((format.cfFormat, bytes));
        }
        if frelease.as_bool() {
            // SAFETY: with `fRelease` set this object owns the medium, and its
            // bytes were copied out above. `ReleaseStgMedium` takes a mutable
            // pointer but only frees the medium's contents.
            unsafe { ReleaseStgMedium(pmedium as *mut STGMEDIUM) };
        }
        Ok(())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        let formats: Vec<FORMATETC> = self
            .entries
            .borrow()
            .iter()
            .map(|(format, _)| format_etc(*format))
            .collect();
        // SAFETY: the array is copied by the enumerator before this returns.
        unsafe { SHCreateStdEnumFmtEtc(&formats) }
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Ref<IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

/// A shared handle to an `IDataObject`: the one this crate builds for a drag,
/// or one a drop target receives from OLE.
#[derive(Clone)]
pub(crate) struct DataObj(IDataObject);

impl DataObj {
    /// A data object carrying `payload` in the private payload format.
    ///
    /// `GlobalSize` may round a block up, so the payload is stored behind a
    /// little-endian `u64` length to survive the round trip exactly.
    pub(crate) fn with_payload(payload: &[u8]) -> DataObj {
        let mut framed = Vec::with_capacity(payload.len() + LENGTH_PREFIX);
        framed.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        framed.extend_from_slice(payload);
        let object = DataObject {
            entries: RefCell::new(vec![(payload_format(), framed)]),
        };
        DataObj(object.into())
    }

    pub(super) fn from_com(object: IDataObject) -> DataObj {
        DataObj(object)
    }

    pub(super) fn com(&self) -> &IDataObject {
        &self.0
    }

    fn has(&self, format: u16) -> bool {
        let query = format_etc(format);
        // SAFETY: `query` outlives the call; the object only reads it.
        unsafe { self.0.QueryGetData(&query) }.is_ok()
    }

    fn read(&self, format: u16) -> Option<Vec<u8>> {
        let query = format_etc(format);
        // SAFETY: `query` outlives the call.
        let mut medium = unsafe { self.0.GetData(&query) }.ok()?;
        let bytes = if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            // SAFETY: `tymed` says the union holds an `HGLOBAL`.
            hglobal::read(unsafe { medium.u.hGlobal })
        } else {
            None
        };
        // SAFETY: `GetData` transferred ownership of the medium to us.
        unsafe { ReleaseStgMedium(&mut medium) };
        bytes
    }

    /// Whether the object carries an app payload.
    pub(crate) fn has_payload(&self) -> bool {
        self.has(payload_format())
    }

    /// The app payload, if present.
    pub(crate) fn payload(&self) -> Option<Vec<u8>> {
        let framed = self.read(payload_format())?;
        let (prefix, rest) = framed.split_at_checked(LENGTH_PREFIX)?;
        let length = u64::from_le_bytes(prefix.try_into().ok()?);
        rest.get(..usize::try_from(length).ok()?)
            .map(<[u8]>::to_vec)
    }

    /// Whether the object carries a `CF_HDROP` file list.
    pub(crate) fn has_hdrop(&self) -> bool {
        self.has(CF_HDROP.0)
    }

    /// The raw `DROPFILES` block of a `CF_HDROP`, if present.
    pub(crate) fn hdrop(&self) -> Option<Vec<u8>> {
        self.read(CF_HDROP.0)
    }
}

#[cfg(test)]
mod tests;
