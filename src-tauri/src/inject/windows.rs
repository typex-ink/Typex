//! Native Windows text injection using `SendInput` (06 section 7.5).

use crate::error::{ErrorCode, Result, TypexError};
use std::marker::PhantomData;
use std::time::Duration;
use windows::Win32::Foundation::{
    ERROR_SUCCESS, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, SetLastError,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, HWND_MESSAGE, MSG, PM_REMOVE, PeekMessageW,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::w;

const CF_BITMAP_FORMAT: u32 = 2;
const CF_METAFILEPICT_FORMAT: u32 = 3;
const CF_DIB_FORMAT: u32 = 8;
const CF_PALETTE_FORMAT: u32 = 9;
const CF_UNICODETEXT_FORMAT: u32 = 13;
const CF_ENHMETAFILE_FORMAT: u32 = 14;
const CF_HDROP_FORMAT: u32 = 15;
const CF_DIBV5_FORMAT: u32 = 17;
const CF_OWNERDISPLAY_FORMAT: u32 = 0x0080;
const CF_DSPBITMAP_FORMAT: u32 = 0x0082;
const CF_DSPMETAFILEPICT_FORMAT: u32 = 0x0083;
const CF_DSPENHMETAFILE_FORMAT: u32 = 0x008e;

const OPEN_RETRY_DELAYS_MS: [u64; 6] = [5, 5, 10, 20, 40, 50];
const MAX_CLIPBOARD_FORMATS: usize = 4_096;
const MAX_FORMAT_BYTES: usize = 128 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;
const MAX_EMPTY_SELECTION_METADATA_BYTES: usize = 64 * 1024;
const RESTORE_DELAY_MS: u64 = 200;

const KEYUP: u32 = 0x0002;
const UNICODE: u32 = 0x0004;
const VK_CONTROL_CODE: u16 = 0x11;
const VK_C_CODE: u16 = 0x43;
const VK_RETURN_CODE: u16 = 0x0D;
const VK_V_CODE: u16 = 0x56;
const TYPEX_INPUT_TAG: usize = 0x0054_5950_4558;

fn retry_bounded<T, E>(
    mut operation: impl FnMut() -> std::result::Result<T, E>,
    mut wait: impl FnMut(Duration),
) -> std::result::Result<T, E> {
    let mut last_error = match operation() {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };

    for delay_ms in OPEN_RETRY_DELAYS_MS {
        wait(Duration::from_millis(delay_ms));
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn clipboard_error(operation: &'static str, detail: impl std::fmt::Display) -> TypexError {
    TypexError::new(
        ErrorCode::Internal,
        format!("Windows clipboard {operation} failed: {detail}"),
    )
}

fn clipboard_sequence(operation: &'static str) -> Result<u32> {
    let sequence = unsafe { GetClipboardSequenceNumber() };
    require_clipboard_sequence(sequence, operation)
}

fn clipboard_owner_id() -> Option<isize> {
    unsafe { GetClipboardOwner() }
        .ok()
        .filter(|owner| !owner.is_invalid())
        .map(|owner| owner.0 as isize)
}

fn require_clipboard_sequence(sequence: u32, operation: &'static str) -> Result<u32> {
    if sequence == 0 {
        Err(clipboard_error(operation, "sequence number unavailable"))
    } else {
        Ok(sequence)
    }
}

struct ClipboardOwner {
    hwnd: HWND,
    _not_send: PhantomData<*mut ()>,
}

impl ClipboardOwner {
    fn new() -> Result<Self> {
        // EmptyClipboard requires a valid owner HWND before SetClipboardData can succeed.
        // A message-only STATIC window is lightweight and belongs to this worker thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Typex Clipboard Owner"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|error| clipboard_error("create owner window", error))?;

        Ok(Self {
            hwnd,
            _not_send: PhantomData,
        })
    }

    fn pump_messages(&self) {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, Some(self.hwnd), 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

impl Drop for ClipboardOwner {
    fn drop(&mut self) {
        if unsafe { DestroyWindow(self.hwnd) }.is_err() {
            tracing::warn!("failed to destroy Windows clipboard owner window");
        }
    }
}

struct ClipboardGuard {
    _not_send: PhantomData<*mut ()>,
}

impl ClipboardGuard {
    fn open(owner: HWND) -> Result<Self> {
        retry_bounded(|| unsafe { OpenClipboard(Some(owner)) }, std::thread::sleep)
            .map(|()| Self {
                _not_send: PhantomData,
            })
            .map_err(|error| clipboard_error("open", error))
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        if unsafe { CloseClipboard() }.is_err() {
            tracing::warn!("failed to close Windows clipboard");
        }
    }
}

struct OwnedGlobal(HGLOBAL);

impl OwnedGlobal {
    fn allocate(size: usize, format: u32) -> Result<Self> {
        if size == 0 {
            return Err(clipboard_error(
                "allocate",
                format_args!("format {format} has no data"),
            ));
        }
        unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, size) }
            .map(Self)
            .map_err(|error| clipboard_error("allocate", error))
    }

    fn from_bytes(bytes: &[u8], format: u32) -> Result<Self> {
        let memory = Self::allocate(bytes.len(), format)?;
        let destination = unsafe { GlobalLock(memory.0) }.cast::<u8>();
        if destination.is_null() {
            return Err(clipboard_error("lock", format_args!("format {format}")));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
            let _ = GlobalUnlock(memory.0);
        }
        Ok(memory)
    }

    fn copy_from(source: HGLOBAL, size: usize, format: u32) -> Result<Self> {
        let memory = Self::allocate(size, format)?;
        let source_pointer = unsafe { GlobalLock(source) }.cast::<u8>();
        if source_pointer.is_null() {
            return Err(clipboard_error(
                "lock source",
                format_args!("format {format}"),
            ));
        }

        let destination = unsafe { GlobalLock(memory.0) }.cast::<u8>();
        if destination.is_null() {
            unsafe {
                let _ = GlobalUnlock(source);
            }
            return Err(clipboard_error(
                "lock destination",
                format_args!("format {format}"),
            ));
        }

        unsafe {
            std::ptr::copy_nonoverlapping(source_pointer, destination, size);
            let _ = GlobalUnlock(memory.0);
            let _ = GlobalUnlock(source);
        }
        Ok(memory)
    }

    fn transfer(self, format: u32) -> Result<()> {
        let handle = HANDLE(self.0.0);
        match unsafe { SetClipboardData(format, Some(handle)) } {
            Ok(_) => {
                std::mem::forget(self);
                Ok(())
            }
            Err(error) => Err(clipboard_error("set data", error)),
        }
    }
}

impl Drop for OwnedGlobal {
    fn drop(&mut self) {
        unsafe {
            let _ = GlobalFree(Some(self.0));
        }
    }
}

struct CapturedFormat {
    format: u32,
    memory: OwnedGlobal,
}

struct ClipboardSnapshot {
    formats: Vec<CapturedFormat>,
}

fn is_common_hglobal_format(format: u32) -> bool {
    matches!(
        format,
        CF_UNICODETEXT_FORMAT | CF_DIB_FORMAT | CF_DIBV5_FORMAT | CF_HDROP_FORMAT
    )
}

fn is_hglobal_candidate(format: u32) -> bool {
    !matches!(
        format,
        CF_BITMAP_FORMAT
            | CF_METAFILEPICT_FORMAT
            | CF_PALETTE_FORMAT
            | CF_ENHMETAFILE_FORMAT
            | CF_OWNERDISPLAY_FORMAT
            | CF_DSPBITMAP_FORMAT
            | CF_DSPMETAFILEPICT_FORMAT
            | CF_DSPENHMETAFILE_FORMAT
    )
}

fn capture_snapshot() -> Result<ClipboardSnapshot> {
    let mut formats = Vec::new();
    let mut current = 0;
    let mut total_bytes = 0usize;

    loop {
        unsafe {
            SetLastError(ERROR_SUCCESS);
        }
        let next = unsafe { EnumClipboardFormats(current) };
        if next == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_SUCCESS {
                return Err(clipboard_error(
                    "enumerate formats",
                    format_args!("Win32 error {}", error.0),
                ));
            }
            break;
        }
        current = next;
        if formats.len() >= MAX_CLIPBOARD_FORMATS {
            return Err(clipboard_error(
                "capture",
                "clipboard contains too many formats",
            ));
        }
        if !is_hglobal_candidate(current) {
            continue;
        }

        let handle = match unsafe { GetClipboardData(current) } {
            Ok(handle) => handle,
            Err(error) if is_common_hglobal_format(current) => {
                return Err(clipboard_error("read common format", error));
            }
            Err(_) => {
                tracing::debug!(format = current, "skipping unavailable clipboard format");
                continue;
            }
        };
        let global = HGLOBAL(handle.0);
        unsafe {
            SetLastError(ERROR_SUCCESS);
        }
        let size = unsafe { GlobalSize(global) };
        let size_error = unsafe { GetLastError() };
        if size == 0 {
            if is_common_hglobal_format(current) {
                return Err(clipboard_error(
                    "measure common format",
                    format_args!("format {current}, Win32 error {}", size_error.0),
                ));
            }
            tracing::debug!(
                format = current,
                win32_error = size_error.0,
                "skipping non-HGLOBAL clipboard format"
            );
            continue;
        }
        if size > MAX_FORMAT_BYTES {
            return Err(clipboard_error(
                "capture",
                format_args!("format {current} exceeds the per-format size limit"),
            ));
        }
        total_bytes = total_bytes
            .checked_add(size)
            .ok_or_else(|| clipboard_error("capture", "clipboard snapshot size overflow"))?;
        if total_bytes > MAX_SNAPSHOT_BYTES {
            return Err(clipboard_error(
                "capture",
                "clipboard snapshot exceeds the total size limit",
            ));
        }

        formats.push(CapturedFormat {
            format: current,
            memory: OwnedGlobal::copy_from(global, size, current)?,
        });
    }

    Ok(ClipboardSnapshot { formats })
}

fn unicode_clipboard_bytes(text: &str) -> Result<Vec<u8>> {
    let unit_count = text.encode_utf16().count();
    let byte_count = unit_count
        .checked_add(1)
        .and_then(|count| count.checked_mul(std::mem::size_of::<u16>()))
        .ok_or_else(|| clipboard_error("encode text", "text is too large"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_count)
        .map_err(|error| clipboard_error("encode text", error))?;
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    Ok(bytes)
}

fn replace_unicode_text_open(text: &str) -> Result<()> {
    let encoded = unicode_clipboard_bytes(text)?;
    let text_memory = OwnedGlobal::from_bytes(&encoded, CF_UNICODETEXT_FORMAT)?;
    unsafe { EmptyClipboard() }.map_err(|error| clipboard_error("empty for replace", error))?;
    text_memory.transfer(CF_UNICODETEXT_FORMAT)
}

fn unicode_clipboard_unit_count(size: usize) -> Result<usize> {
    if size == 0 || !size.is_multiple_of(std::mem::size_of::<u16>()) {
        return Err(clipboard_error(
            "read text",
            "CF_UNICODETEXT has an invalid byte length",
        ));
    }
    if size > MAX_FORMAT_BYTES {
        return Err(clipboard_error(
            "read text",
            "CF_UNICODETEXT exceeds the 128 MiB size limit",
        ));
    }
    Ok(size / std::mem::size_of::<u16>())
}

fn read_unicode_clipboard_open() -> Result<Option<String>> {
    let handle = match unsafe { GetClipboardData(CF_UNICODETEXT_FORMAT) } {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };
    let global = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(global) };
    let unit_count = unicode_clipboard_unit_count(size)?;

    let pointer = unsafe { GlobalLock(global) }.cast::<u16>();
    if pointer.is_null() {
        return Err(clipboard_error("lock text", "CF_UNICODETEXT"));
    }
    let units = unsafe { std::slice::from_raw_parts(pointer, unit_count) };
    let length = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let decoded =
        String::from_utf16(&units[..length]).map_err(|error| clipboard_error("decode text", error));
    unsafe {
        let _ = GlobalUnlock(global);
    }
    decoded.map(Some)
}

fn read_hglobal_bytes_open(format: u32, max_bytes: usize) -> Option<Vec<u8>> {
    if format == 0 {
        return None;
    }
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    let global = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(global) };
    if size == 0 || size > max_bytes {
        return None;
    }

    let pointer = unsafe { GlobalLock(global) }.cast::<u8>();
    if pointer.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer, size) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(global);
    }
    Some(bytes)
}

#[derive(serde::Deserialize)]
struct VscodeEditorData {
    version: u8,
    #[serde(rename = "isFromEmptySelection")]
    is_from_empty_selection: bool,
}

fn vscode_metadata_marks_empty_selection(metadata: &[u8]) -> bool {
    if metadata.len() > MAX_EMPTY_SELECTION_METADATA_BYTES {
        return false;
    }
    let metadata = metadata
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(&[][..], |last| &metadata[..=last]);
    serde_json::from_slice::<VscodeEditorData>(metadata)
        .map(|data| data.version == 1 && data.is_from_empty_selection)
        .unwrap_or(false)
}

fn selection_from_clipboard_payload(
    text: Option<String>,
    vscode_editor_data: Option<&[u8]>,
) -> Option<String> {
    let text = text.filter(|value| !value.is_empty());
    if vscode_editor_data.is_some_and(vscode_metadata_marks_empty_selection) {
        None
    } else {
        text
    }
}

struct ClipboardSelectionObservation {
    sequence: u32,
    owner_id: Option<isize>,
    text: Option<String>,
    vscode_editor_data: Option<Vec<u8>>,
}

fn resolve_clipboard_selection_observation(
    detected_sequence: u32,
    detected_owner_id: Option<isize>,
    observation: ClipboardSelectionObservation,
) -> (u32, Option<String>) {
    let same_copy_owner = detected_owner_id.is_some() && detected_owner_id == observation.owner_id;
    if observation.sequence != detected_sequence && !same_copy_owner {
        // Something changed between polling and the atomic clipboard read. Keeping the earlier
        // sequence as the restore condition guarantees that this newer clipboard is not replaced.
        return (detected_sequence, None);
    }
    (
        observation.sequence,
        selection_from_clipboard_payload(
            observation.text,
            observation.vscode_editor_data.as_deref(),
        ),
    )
}

fn read_detected_clipboard_selection(
    restore_sequence: &mut u32,
    detected_sequence: u32,
    detected_owner_id: Option<isize>,
    read: impl FnOnce() -> Result<ClipboardSelectionObservation>,
) -> Result<Option<String>> {
    // Record the detected copy before opening or decoding its payload. If payload access fails,
    // the caller may still restore against this exact sequence without overwriting a later change.
    *restore_sequence = detected_sequence;
    let observation = read()?;
    let (observed_sequence, selection) =
        resolve_clipboard_selection_observation(detected_sequence, detected_owner_id, observation);
    *restore_sequence = observed_sequence;
    Ok(selection)
}

fn read_selection_clipboard(owner: HWND) -> Result<ClipboardSelectionObservation> {
    let _clipboard = ClipboardGuard::open(owner)?;
    let text = read_unicode_clipboard_open()?;
    // This is the editor-core metadata used by VS Code and Monaco integrations. Registering an
    // existing format is idempotent and does not mutate clipboard contents or its sequence.
    let vscode_format = unsafe { RegisterClipboardFormatW(w!("vscode-editor-data")) };
    let vscode_editor_data =
        read_hglobal_bytes_open(vscode_format, MAX_EMPTY_SELECTION_METADATA_BYTES);
    let sequence = clipboard_sequence("observe selection")?;
    Ok(ClipboardSelectionObservation {
        sequence,
        owner_id: clipboard_owner_id(),
        text,
        vscode_editor_data,
    })
}

fn settle_written_sequence(owner: HWND, open_sequence: u32) -> Result<u32> {
    let closed_sequence = clipboard_sequence("record closed write")?;
    if closed_sequence == open_sequence {
        return Ok(open_sequence);
    }

    // Windows may publish one final sequence transition when CloseClipboard completes. That does
    // not change the owner installed by EmptyClipboard; any external replacement does. Check the
    // owner before accepting the rollover, then let every later operation recheck the sequence.
    if !matches!(unsafe { GetClipboardOwner() }, Ok(current) if current == owner) {
        return Err(clipboard_error(
            "verify closed write",
            "clipboard changed immediately after Typex wrote it",
        ));
    }
    Ok(closed_sequence)
}

fn restore_snapshot_open(snapshot: ClipboardSnapshot) -> Result<()> {
    unsafe { EmptyClipboard() }.map_err(|error| clipboard_error("empty for restore", error))?;

    let mut first_error = None;
    for captured in snapshot.formats {
        let format = captured.format;
        if let Err(error) = captured.memory.transfer(format) {
            tracing::warn!(
                format,
                error_code = ?error.code,
                "failed to restore Windows clipboard format"
            );
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreDisposition {
    Restored,
    ClipboardChanged,
}

fn restore_disposition(written_sequence: u32, current_sequence: u32) -> RestoreDisposition {
    if written_sequence == current_sequence {
        RestoreDisposition::Restored
    } else {
        RestoreDisposition::ClipboardChanged
    }
}

fn clipboard_sequence_changed(written_sequence: u32, current_sequence: u32) -> bool {
    written_sequence != current_sequence
}

trait ClipboardTransactionPort {
    type Snapshot;

    fn begin(&mut self, text: &str) -> Result<(Self::Snapshot, u32)>;
    fn is_unchanged(&mut self, written_sequence: u32) -> Result<bool>;
    fn restore_if_unchanged(
        &mut self,
        snapshot: Self::Snapshot,
        written_sequence: u32,
    ) -> Result<RestoreDisposition>;
}

trait ClipboardReplacePort {
    fn replace(&mut self, text: &str) -> Result<()>;
}

struct NativeClipboardPort {
    owner: ClipboardOwner,
}

impl NativeClipboardPort {
    fn new() -> Result<Self> {
        Ok(Self {
            owner: ClipboardOwner::new()?,
        })
    }

    fn read_selection(&self) -> Result<ClipboardSelectionObservation> {
        read_selection_clipboard(self.owner.hwnd)
    }

    fn pump_owner_messages(&self) {
        self.owner.pump_messages();
    }
}

impl ClipboardReplacePort for NativeClipboardPort {
    fn replace(&mut self, text: &str) -> Result<()> {
        let _clipboard = ClipboardGuard::open(self.owner.hwnd)?;
        replace_unicode_text_open(text)
    }
}

impl ClipboardTransactionPort for NativeClipboardPort {
    type Snapshot = ClipboardSnapshot;

    fn begin(&mut self, text: &str) -> Result<(Self::Snapshot, u32)> {
        let encoded = unicode_clipboard_bytes(text)?;
        let text_memory = OwnedGlobal::from_bytes(&encoded, CF_UNICODETEXT_FORMAT)?;
        let clipboard = ClipboardGuard::open(self.owner.hwnd)?;
        let snapshot = capture_snapshot()?;

        unsafe { EmptyClipboard() }.map_err(|error| clipboard_error("empty for paste", error))?;
        if let Err(write_error) = text_memory.transfer(CF_UNICODETEXT_FORMAT) {
            if let Err(restore_error) = restore_snapshot_open(snapshot) {
                tracing::warn!(
                    error_code = ?restore_error.code,
                    "failed to restore Windows clipboard after a write failure"
                );
            }
            return Err(write_error);
        }

        let open_sequence = match clipboard_sequence("record write") {
            Ok(sequence) => sequence,
            Err(sequence_error) => {
                if let Err(restore_error) = restore_snapshot_open(snapshot) {
                    tracing::warn!(
                        error_code = ?restore_error.code,
                        "failed to restore Windows clipboard after sequence lookup failed"
                    );
                }
                return Err(sequence_error);
            }
        };
        drop(clipboard);
        let written_sequence = settle_written_sequence(self.owner.hwnd, open_sequence)?;
        Ok((snapshot, written_sequence))
    }

    fn is_unchanged(&mut self, written_sequence: u32) -> Result<bool> {
        Ok(clipboard_sequence("check before paste")? == written_sequence)
    }

    fn restore_if_unchanged(
        &mut self,
        snapshot: Self::Snapshot,
        written_sequence: u32,
    ) -> Result<RestoreDisposition> {
        let _clipboard = ClipboardGuard::open(self.owner.hwnd)?;
        let current_sequence = clipboard_sequence("check before restore")?;
        let disposition = restore_disposition(written_sequence, current_sequence);
        if disposition == RestoreDisposition::ClipboardChanged {
            return Ok(disposition);
        }
        restore_snapshot_open(snapshot)?;
        Ok(RestoreDisposition::Restored)
    }
}

fn execute_clipboard_replace<P: ClipboardReplacePort>(port: &mut P, text: &str) -> Result<()> {
    port.replace(text)
}

pub(crate) fn replace_clipboard_text(text: &str) -> Result<()> {
    let mut clipboard = NativeClipboardPort::new()?;
    execute_clipboard_replace(&mut clipboard, text)
}

/// Reads the current selection through Ctrl+C without losing rich clipboard formats. The
/// original snapshot is restored only when neither the user nor a clipboard manager replaced
/// the value written by this transaction.
pub(crate) fn read_selected_text_to(
    target: Option<&crate::platform::focus::FocusTarget>,
) -> Result<Option<String>> {
    const SENTINEL: &str = "\u{200B}typex-selection-probe\u{200B}";
    const POLL_ATTEMPTS: usize = 10;
    const POLL_DELAY_MS: u64 = 30;

    if !crate::platform::focus::captured_target_is_current(target) {
        return Err(TypexError::new(
            ErrorCode::NoFocus,
            "foreground target changed before clipboard selection read",
        ));
    }

    let mut clipboard = NativeClipboardPort::new()?;
    let (snapshot, written_sequence) = clipboard.begin(SENTINEL)?;
    let mut restore_sequence = written_sequence;
    let outcome = (|| {
        send_ctrl_c_to(target)?;
        for _ in 0..POLL_ATTEMPTS {
            std::thread::sleep(Duration::from_millis(POLL_DELAY_MS));
            clipboard.pump_owner_messages();
            if !crate::platform::focus::captured_target_is_current(target) {
                return Err(TypexError::new(
                    ErrorCode::NoFocus,
                    "foreground target changed during clipboard selection read",
                ));
            }

            let current_sequence = clipboard_sequence("poll selection copy")?;
            if !clipboard_sequence_changed(written_sequence, current_sequence) {
                continue;
            }
            let detected_owner_id = clipboard_owner_id();
            return read_detected_clipboard_selection(
                &mut restore_sequence,
                current_sequence,
                detected_owner_id,
                || clipboard.read_selection(),
            );
        }

        if !crate::platform::focus::captured_target_is_current(target) {
            return Err(TypexError::new(
                ErrorCode::NoFocus,
                "foreground target changed during clipboard selection read",
            ));
        }
        Ok(None)
    })();

    let restore_result = clipboard.restore_if_unchanged(snapshot, restore_sequence);
    match (outcome, restore_result) {
        (Ok(selection), Ok(_)) => Ok(selection),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(restore_error)) => {
            tracing::warn!(
                error_code = ?restore_error.code,
                "failed to restore Windows clipboard after selection read"
            );
            Err(error)
        }
    }
}

fn execute_clipboard_paste<P>(
    port: &mut P,
    text: &str,
    paste_delay_ms: u64,
    mut send_paste: impl FnMut() -> PasteDispatchOutcome,
    mut wait: impl FnMut(Duration),
) -> Result<()>
where
    P: ClipboardTransactionPort,
{
    let (snapshot, written_sequence) = port.begin(text)?;
    wait(Duration::from_millis(paste_delay_ms.max(10)));

    let dispatch = if port.is_unchanged(written_sequence)? {
        send_paste()
    } else {
        PasteDispatchOutcome::not_dispatched(Err(TypexError::new(
            ErrorCode::Internal,
            "clipboard changed before paste; refusing to paste unrelated content",
        )))
    };
    if dispatch.result.is_ok() || dispatch.action_key_accepted {
        wait(Duration::from_millis(RESTORE_DELAY_MS));
    }

    if let Err(error) = port.restore_if_unchanged(snapshot, written_sequence) {
        tracing::warn!(
            error_code = ?error.code,
            "failed to restore Windows clipboard after paste"
        );
    }
    dispatch.result
}

pub fn paste_text(text: &str, paste_delay_ms: u64) -> Result<()> {
    paste_text_to(text, paste_delay_ms, None)
}

pub fn paste_text_to(
    text: &str,
    paste_delay_ms: u64,
    target: Option<&crate::platform::focus::FocusTarget>,
) -> Result<()> {
    let mut clipboard = NativeClipboardPort::new()?;
    execute_clipboard_paste(
        &mut clipboard,
        text,
        paste_delay_ms,
        || send_ctrl_v_for_paste(target),
        std::thread::sleep,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeyStroke {
    vk: u16,
    scan: u16,
    flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyIdentity {
    Virtual { vk: u16, scan: u16, flags: u32 },
    Unicode(u16),
}

impl KeyStroke {
    const fn virtual_key(vk: u16, key_up: bool) -> Self {
        Self {
            vk,
            scan: 0,
            flags: if key_up { KEYUP } else { 0 },
        }
    }

    const fn unicode(scan: u16, key_up: bool) -> Self {
        Self {
            vk: 0,
            scan,
            flags: UNICODE | if key_up { KEYUP } else { 0 },
        }
    }

    const fn identity(self) -> KeyIdentity {
        if self.flags & UNICODE != 0 {
            KeyIdentity::Unicode(self.scan)
        } else {
            KeyIdentity::Virtual {
                vk: self.vk,
                scan: self.scan,
                flags: self.flags & !KEYUP,
            }
        }
    }

    const fn is_key_up(self) -> bool {
        self.flags & KEYUP != 0
    }

    const fn into_key_up(mut self) -> Self {
        self.flags |= KEYUP;
        self
    }

    fn into_input(self) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(self.vk),
                    wScan: self.scan,
                    dwFlags: KEYBD_EVENT_FLAGS(self.flags),
                    time: 0,
                    dwExtraInfo: TYPEX_INPUT_TAG,
                },
            },
        }
    }
}

fn cleanup_keyups_for_accepted_prefix(strokes: &[KeyStroke], accepted: usize) -> Vec<KeyStroke> {
    let mut pressed = Vec::<(KeyIdentity, KeyStroke)>::new();

    for stroke in strokes.iter().copied().take(accepted) {
        let identity = stroke.identity();
        if stroke.is_key_up() {
            if let Some(index) = pressed
                .iter()
                .rposition(|(pressed_identity, _)| *pressed_identity == identity)
            {
                pressed.remove(index);
            }
        } else if !pressed
            .iter()
            .any(|(pressed_identity, _)| *pressed_identity == identity)
        {
            pressed.push((identity, stroke));
        }
    }

    pressed
        .into_iter()
        .rev()
        .map(|(_, stroke)| stroke.into_key_up())
        .collect()
}

fn ctrl_v_strokes() -> Vec<KeyStroke> {
    vec![
        KeyStroke::virtual_key(VK_CONTROL_CODE, false),
        KeyStroke::virtual_key(VK_V_CODE, false),
        KeyStroke::virtual_key(VK_V_CODE, true),
        KeyStroke::virtual_key(VK_CONTROL_CODE, true),
    ]
}

fn ctrl_c_strokes() -> Vec<KeyStroke> {
    vec![
        KeyStroke::virtual_key(VK_CONTROL_CODE, false),
        KeyStroke::virtual_key(VK_C_CODE, false),
        KeyStroke::virtual_key(VK_C_CODE, true),
        KeyStroke::virtual_key(VK_CONTROL_CODE, true),
    ]
}

fn unicode_strokes(text: &str) -> Vec<KeyStroke> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut strokes = Vec::with_capacity(normalized.encode_utf16().count() * 2);
    for ch in normalized.chars() {
        if ch == '\n' {
            strokes.push(KeyStroke::virtual_key(VK_RETURN_CODE, false));
            strokes.push(KeyStroke::virtual_key(VK_RETURN_CODE, true));
            continue;
        }
        let mut encoded = [0u16; 2];
        for unit in ch.encode_utf16(&mut encoded).iter().copied() {
            strokes.push(KeyStroke::unicode(unit, false));
            strokes.push(KeyStroke::unicode(unit, true));
        }
    }
    strokes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputAccess {
    Write,
    ReadOnlyAllowed,
}

fn ensure_foreground_window(
    target: Option<&crate::platform::focus::FocusTarget>,
    access: InputAccess,
) -> Result<()> {
    if target.is_some_and(|target| !target.is_current()) {
        return Err(TypexError::new(
            ErrorCode::NoFocus,
            "foreground target changed before SendInput",
        ));
    }
    if !crate::platform::windows::foreground_window_exists() {
        return Err(TypexError::new(
            ErrorCode::NoFocus,
            "Windows has no foreground window",
        ));
    }
    if !crate::platform::windows::foreground_has_keyboard_focus() {
        return Err(TypexError::new(
            ErrorCode::NoFocus,
            "Windows foreground thread has no enabled keyboard focus",
        ));
    }
    if access == InputAccess::Write
        && crate::platform::windows::foreground_focus_is_known_read_only()
    {
        return Err(TypexError::new(
            ErrorCode::NoFocus,
            "Windows keyboard focus is a read-only text control",
        ));
    }
    match crate::platform::windows::can_inject_foreground() {
        Ok(true) => Ok(()),
        Ok(false) => Err(TypexError::new(
            ErrorCode::InjectionBlocked,
            "UIPI blocks injection into the foreground process",
        )),
        Err(classification) => Err(TypexError::new(
            ErrorCode::InjectionBlocked,
            format!("could not establish foreground integrity: {classification}"),
        )),
    }
}

struct SendInputAttempt {
    accepted: usize,
    os_error: Option<String>,
}

struct SendInputBatchOutcome {
    result: Result<()>,
    accepted: usize,
}

struct PasteDispatchOutcome {
    result: Result<()>,
    action_key_accepted: bool,
}

impl PasteDispatchOutcome {
    fn not_dispatched(result: Result<()>) -> Self {
        Self {
            result,
            action_key_accepted: false,
        }
    }
}

#[cfg(test)]
impl SendInputAttempt {
    fn from_accepted(accepted: usize) -> Self {
        Self {
            accepted,
            os_error: None,
        }
    }
}

fn native_send_input(strokes: &[KeyStroke]) -> SendInputAttempt {
    let inputs: Vec<INPUT> = strokes.iter().copied().map(KeyStroke::into_input).collect();
    // SAFETY: INPUT points to an initialized live slice; cbSize matches the ABI type.
    let accepted = (unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) }) as usize;
    let os_error = (accepted == 0).then(|| std::io::Error::last_os_error().to_string());
    SendInputAttempt { accepted, os_error }
}

#[cfg(test)]
fn send_input_batch_with(
    strokes: &[KeyStroke],
    send_input: impl FnMut(&[KeyStroke]) -> SendInputAttempt,
) -> Result<()> {
    send_input_batch_outcome_with(strokes, send_input).result
}

fn send_input_batch_outcome_with(
    strokes: &[KeyStroke],
    mut send_input: impl FnMut(&[KeyStroke]) -> SendInputAttempt,
) -> SendInputBatchOutcome {
    let attempt = send_input(strokes);
    let sent = attempt.accepted;
    if sent == strokes.len() {
        return SendInputBatchOutcome {
            result: Ok(()),
            accepted: sent,
        };
    }

    let original_error = if sent == 0 {
        TypexError::new(
            ErrorCode::InjectionBlocked,
            format!(
                "SendInput was blocked (UIPI or policy): {}",
                attempt
                    .os_error
                    .as_deref()
                    .unwrap_or("unknown Windows error")
            ),
        )
    } else {
        TypexError::new(
            ErrorCode::InjectionBlocked,
            format!(
                "SendInput accepted {sent}/{} events; refusing an unsafe fallback retry",
                strokes.len()
            ),
        )
    };

    let cleanup = cleanup_keyups_for_accepted_prefix(strokes, sent);
    if !cleanup.is_empty() {
        let cleanup_sent = send_input(&cleanup).accepted;
        if cleanup_sent != cleanup.len() {
            tracing::warn!(
                accepted_count = cleanup_sent,
                expected_count = cleanup.len(),
                "Windows SendInput key-release cleanup was incomplete"
            );
        }
    }

    SendInputBatchOutcome {
        result: Err(original_error),
        accepted: sent,
    }
}

fn send_strokes_with_outcome(
    strokes: Vec<KeyStroke>,
    target: Option<&crate::platform::focus::FocusTarget>,
    access: InputAccess,
) -> SendInputBatchOutcome {
    if let Err(error) = ensure_foreground_window(target, access) {
        return SendInputBatchOutcome {
            result: Err(error),
            accepted: 0,
        };
    }
    if strokes.is_empty() {
        return SendInputBatchOutcome {
            result: Ok(()),
            accepted: 0,
        };
    }
    // Integrity lookup above is not atomic with SendInput. Revalidate the opaque session target at
    // the last possible boundary so a focus switch cannot redirect the batch to another window.
    if target.is_some_and(|target| !target.is_current()) {
        return SendInputBatchOutcome {
            result: Err(TypexError::new(
                ErrorCode::NoFocus,
                "foreground target changed immediately before SendInput",
            )),
            accepted: 0,
        };
    }
    if !crate::platform::windows::foreground_has_keyboard_focus() {
        return SendInputBatchOutcome {
            result: Err(TypexError::new(
                ErrorCode::NoFocus,
                "Windows foreground thread lost keyboard focus before SendInput",
            )),
            accepted: 0,
        };
    }
    if access == InputAccess::Write
        && crate::platform::windows::foreground_focus_is_known_read_only()
    {
        return SendInputBatchOutcome {
            result: Err(TypexError::new(
                ErrorCode::NoFocus,
                "Windows keyboard focus became read-only before SendInput",
            )),
            accepted: 0,
        };
    }
    send_input_batch_outcome_with(&strokes, native_send_input)
}

fn send_strokes(
    strokes: Vec<KeyStroke>,
    target: Option<&crate::platform::focus::FocusTarget>,
    access: InputAccess,
) -> Result<()> {
    send_strokes_with_outcome(strokes, target, access).result
}

fn send_ctrl_v_for_paste(
    target: Option<&crate::platform::focus::FocusTarget>,
) -> PasteDispatchOutcome {
    const CTRL_V_ACTION_KEY_INDEX: usize = 1;
    let outcome = send_strokes_with_outcome(ctrl_v_strokes(), target, InputAccess::Write);
    PasteDispatchOutcome {
        action_key_accepted: outcome.accepted > CTRL_V_ACTION_KEY_INDEX,
        result: outcome.result,
    }
}

pub fn send_ctrl_v() -> Result<()> {
    send_ctrl_v_to(None)
}

pub fn send_ctrl_v_to(target: Option<&crate::platform::focus::FocusTarget>) -> Result<()> {
    send_strokes(ctrl_v_strokes(), target, InputAccess::Write)
}

pub fn send_ctrl_c() -> Result<()> {
    send_ctrl_c_to(None)
}

pub(crate) fn send_ctrl_c_to(target: Option<&crate::platform::focus::FocusTarget>) -> Result<()> {
    send_strokes(ctrl_c_strokes(), target, InputAccess::ReadOnlyAllowed)
}

pub fn send_unicode(text: &str) -> Result<()> {
    send_unicode_to(text, None)
}

pub fn send_unicode_to(
    text: &str,
    target: Option<&crate::platform::focus::FocusTarget>,
) -> Result<()> {
    send_strokes(unicode_strokes(text), target, InputAccess::Write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TransactionEvent {
        Begin,
        Replace,
        Wait(u64),
        Send,
        Restore,
    }

    struct MockClipboardPort {
        events: Rc<RefCell<Vec<TransactionEvent>>>,
        written_sequence: u32,
        current_sequence: u32,
        restore_fails: bool,
        restored: Rc<Cell<bool>>,
        original_formats: Vec<u32>,
        restored_formats: Vec<u32>,
    }

    impl ClipboardTransactionPort for MockClipboardPort {
        type Snapshot = Vec<u32>;

        fn begin(&mut self, _text: &str) -> Result<(Self::Snapshot, u32)> {
            self.events.borrow_mut().push(TransactionEvent::Begin);
            Ok((self.original_formats.clone(), self.written_sequence))
        }

        fn is_unchanged(&mut self, written_sequence: u32) -> Result<bool> {
            Ok(self.current_sequence == written_sequence)
        }

        fn restore_if_unchanged(
            &mut self,
            snapshot: Self::Snapshot,
            written_sequence: u32,
        ) -> Result<RestoreDisposition> {
            self.events.borrow_mut().push(TransactionEvent::Restore);
            let disposition = restore_disposition(written_sequence, self.current_sequence);
            if disposition == RestoreDisposition::ClipboardChanged {
                return Ok(disposition);
            }
            if self.restore_fails {
                return Err(TypexError::new(ErrorCode::Internal, "mock restore failure"));
            }
            self.restored_formats = snapshot;
            self.restored.set(true);
            Ok(RestoreDisposition::Restored)
        }
    }

    impl ClipboardReplacePort for MockClipboardPort {
        fn replace(&mut self, _text: &str) -> Result<()> {
            self.events.borrow_mut().push(TransactionEvent::Replace);
            Ok(())
        }
    }

    fn mock_port(
        events: Rc<RefCell<Vec<TransactionEvent>>>,
        current_sequence: u32,
        restore_fails: bool,
    ) -> (MockClipboardPort, Rc<Cell<bool>>) {
        let restored = Rc::new(Cell::new(false));
        (
            MockClipboardPort {
                events,
                written_sequence: 41,
                current_sequence,
                restore_fails,
                restored: restored.clone(),
                original_formats: vec![CF_UNICODETEXT_FORMAT, CF_DIB_FORMAT, 0xc000],
                restored_formats: Vec::new(),
            },
            restored,
        )
    }

    #[test]
    fn clipboard_open_retry_is_bounded_and_can_recover_from_contention() {
        let attempts = Cell::new(0usize);
        let waits = RefCell::new(Vec::new());
        let result = retry_bounded(
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                if attempt < 3 { Err("busy") } else { Ok(17) }
            },
            |duration| waits.borrow_mut().push(duration.as_millis() as u64),
        );

        assert_eq!(result, Ok(17));
        assert_eq!(attempts.get(), 3);
        assert_eq!(*waits.borrow(), vec![5, 5]);
    }

    #[test]
    fn clipboard_open_retry_stops_after_the_fixed_budget() {
        let attempts = Cell::new(0usize);
        let waited_ms = Cell::new(0u64);
        let result: std::result::Result<(), &str> = retry_bounded(
            || {
                attempts.set(attempts.get() + 1);
                Err("busy")
            },
            |duration| waited_ms.set(waited_ms.get() + duration.as_millis() as u64),
        );

        assert_eq!(result, Err("busy"));
        assert_eq!(attempts.get(), OPEN_RETRY_DELAYS_MS.len() + 1);
        assert_eq!(waited_ms.get(), OPEN_RETRY_DELAYS_MS.iter().sum::<u64>());
    }

    #[test]
    fn hglobal_snapshot_includes_common_and_registered_formats() {
        for format in [
            CF_UNICODETEXT_FORMAT,
            CF_DIB_FORMAT,
            CF_DIBV5_FORMAT,
            CF_HDROP_FORMAT,
            0xc000,
        ] {
            assert!(is_hglobal_candidate(format), "format {format}");
        }
        for format in [
            CF_BITMAP_FORMAT,
            CF_METAFILEPICT_FORMAT,
            CF_PALETTE_FORMAT,
            CF_ENHMETAFILE_FORMAT,
        ] {
            assert!(!is_hglobal_candidate(format), "format {format}");
        }
    }

    #[test]
    fn clipboard_unicode_encoding_keeps_surrogate_pairs_and_trailing_nul() {
        let bytes = unicode_clipboard_bytes("A\u{1f604}").unwrap();
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let mut expected: Vec<u16> = "A\u{1f604}".encode_utf16().collect();
        expected.push(0);
        assert_eq!(units, expected);
    }

    #[test]
    fn clipboard_sequence_zero_is_never_a_valid_transaction_baseline() {
        assert!(require_clipboard_sequence(0, "test").is_err());
        assert_eq!(require_clipboard_sequence(41, "test").unwrap(), 41);
    }

    #[test]
    fn unicode_clipboard_size_limit_is_bounded_before_scanning() {
        assert_eq!(
            unicode_clipboard_unit_count(MAX_FORMAT_BYTES).unwrap(),
            MAX_FORMAT_BYTES / 2
        );
        assert!(unicode_clipboard_unit_count(0).is_err());
        assert!(unicode_clipboard_unit_count(3).is_err());
        assert!(unicode_clipboard_unit_count(MAX_FORMAT_BYTES + 2).is_err());
    }

    #[test]
    fn missing_empty_selection_metadata_keeps_unicode_selection() {
        let selection = selection_from_clipboard_payload(Some("选中 😀".into()), None);
        assert_eq!(selection.as_deref(), Some("选中 😀"));
    }

    #[test]
    fn vscode_empty_selection_metadata_suppresses_copied_line() {
        let mut metadata =
            br#"{"version":1,"isFromEmptySelection":true,"multicursorText":null}"#.to_vec();
        metadata.extend_from_slice(&[0, 0]);

        let selection =
            selection_from_clipboard_payload(Some("entire line\r\n".into()), Some(&metadata));
        assert_eq!(selection, None);
    }

    #[test]
    fn vscode_nonempty_selection_metadata_keeps_selection() {
        let metadata = br#"{"version":1,"isFromEmptySelection":false}"#;
        let selection = selection_from_clipboard_payload(Some("selected".into()), Some(metadata));
        assert_eq!(selection.as_deref(), Some("selected"));
    }

    #[test]
    fn malformed_or_ambiguous_vscode_metadata_is_conservative() {
        let malformed: [&[u8]; 7] = [
            b"",
            b"{",
            br#"{"version":1}"#,
            br#"{"isFromEmptySelection":true}"#,
            br#"{"version":1,"isFromEmptySelection":"true"}"#,
            br#"{"version":1,"isFromEmptySelection":null}"#,
            br#"{"version":2,"isFromEmptySelection":true}"#,
        ];

        for metadata in malformed {
            let selection =
                selection_from_clipboard_payload(Some("real selection".into()), Some(metadata));
            assert_eq!(selection.as_deref(), Some("real selection"));
        }
    }

    #[test]
    fn oversized_vscode_metadata_is_conservative() {
        let metadata = vec![b' '; MAX_EMPTY_SELECTION_METADATA_BYTES + 1];
        let selection =
            selection_from_clipboard_payload(Some("real selection".into()), Some(&metadata));
        assert_eq!(selection.as_deref(), Some("real selection"));
    }

    #[test]
    fn selection_copy_detection_uses_sequence_even_when_text_matches_snapshot() {
        let old_clipboard_text = "same text";
        assert!(clipboard_sequence_changed(41, 42));
        let selection = selection_from_clipboard_payload(Some(old_clipboard_text.into()), None);
        assert_eq!(selection.as_deref(), Some(old_clipboard_text));
        assert!(!clipboard_sequence_changed(42, 42));
    }

    #[test]
    fn stable_clipboard_observation_returns_selection_and_restore_sequence() {
        let (restore_sequence, selection) = resolve_clipboard_selection_observation(
            42,
            Some(10),
            ClipboardSelectionObservation {
                sequence: 42,
                owner_id: Some(10),
                text: Some("稳定选区 😀".into()),
                vscode_editor_data: None,
            },
        );

        assert_eq!(restore_sequence, 42);
        assert_eq!(selection.as_deref(), Some("稳定选区 😀"));
    }

    #[test]
    fn same_owner_sequence_rollover_keeps_selection_and_updates_restore_sequence() {
        let (restore_sequence, selection) = resolve_clipboard_selection_observation(
            42,
            Some(10),
            ClipboardSelectionObservation {
                sequence: 51,
                owner_id: Some(10),
                text: Some("JetBrains selection".into()),
                vscode_editor_data: None,
            },
        );

        assert_eq!(restore_sequence, 51);
        assert_eq!(selection.as_deref(), Some("JetBrains selection"));
    }

    #[test]
    fn clipboard_owner_change_between_poll_and_read_is_not_consumed_or_restored() {
        let detected_copy_sequence = 42;
        let user_change_sequence = 43;
        let (restore_sequence, selection) = resolve_clipboard_selection_observation(
            detected_copy_sequence,
            Some(10),
            ClipboardSelectionObservation {
                sequence: user_change_sequence,
                owner_id: Some(11),
                text: Some("unrelated clipboard text".into()),
                vscode_editor_data: None,
            },
        );

        assert_eq!(selection, None);
        assert_eq!(restore_sequence, detected_copy_sequence);
        assert_eq!(
            restore_disposition(restore_sequence, user_change_sequence),
            RestoreDisposition::ClipboardChanged
        );
    }

    #[test]
    fn payload_read_failure_keeps_detected_sequence_for_conditional_restore() {
        let mut restore_sequence = 41;
        let error = read_detected_clipboard_selection(&mut restore_sequence, 42, Some(10), || {
            Err(TypexError::new(
                ErrorCode::Internal,
                "mock clipboard payload failure",
            ))
        })
        .unwrap_err();

        assert_eq!(error.message, "mock clipboard payload failure");
        assert_eq!(restore_sequence, 42);
        assert_eq!(
            restore_disposition(restore_sequence, 42),
            RestoreDisposition::Restored
        );
        assert_eq!(
            restore_disposition(restore_sequence, 43),
            RestoreDisposition::ClipboardChanged
        );
    }

    #[test]
    fn sequence_wrap_and_user_change_never_look_unchanged() {
        assert!(clipboard_sequence_changed(u32::MAX, 0));
        assert_eq!(
            restore_disposition(u32::MAX, 0),
            RestoreDisposition::ClipboardChanged
        );
        assert_eq!(
            restore_disposition(u32::MAX, u32::MAX),
            RestoreDisposition::Restored
        );

        let copied_sequence = 100;
        let user_change_sequence = 101;
        assert_eq!(
            restore_disposition(copied_sequence, user_change_sequence),
            RestoreDisposition::ClipboardChanged
        );
    }

    #[test]
    fn direct_clipboard_replace_does_not_start_a_restore_transaction() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let (mut port, restored) = mock_port(events.clone(), 41, false);

        execute_clipboard_replace(&mut port, "fallback text").unwrap();

        assert_eq!(*events.borrow(), vec![TransactionEvent::Replace]);
        assert!(!restored.get());
    }

    #[test]
    fn clipboard_transaction_orders_write_paste_and_restore() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let (mut port, restored) = mock_port(events.clone(), 41, false);
        let send_events = events.clone();
        let wait_events = events.clone();

        execute_clipboard_paste(
            &mut port,
            "private text",
            25,
            || {
                send_events.borrow_mut().push(TransactionEvent::Send);
                PasteDispatchOutcome {
                    result: Ok(()),
                    action_key_accepted: true,
                }
            },
            |duration| {
                wait_events
                    .borrow_mut()
                    .push(TransactionEvent::Wait(duration.as_millis() as u64));
            },
        )
        .unwrap();

        assert!(restored.get());
        assert_eq!(port.restored_formats, port.original_formats);
        assert_eq!(
            *events.borrow(),
            vec![
                TransactionEvent::Begin,
                TransactionEvent::Wait(25),
                TransactionEvent::Send,
                TransactionEvent::Wait(RESTORE_DELAY_MS),
                TransactionEvent::Restore,
            ]
        );
    }

    #[test]
    fn clipboard_transaction_refuses_to_paste_new_clipboard_content() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let (mut port, restored) = mock_port(events.clone(), 42, false);
        let send_events = events.clone();

        let error = execute_clipboard_paste(
            &mut port,
            "private text",
            10,
            || {
                send_events.borrow_mut().push(TransactionEvent::Send);
                PasteDispatchOutcome {
                    result: Ok(()),
                    action_key_accepted: true,
                }
            },
            |_| {},
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::Internal);
        assert!(!restored.get());
        assert!(port.restored_formats.is_empty());
        assert_eq!(
            *events.borrow(),
            vec![TransactionEvent::Begin, TransactionEvent::Restore]
        );
    }

    #[test]
    fn restore_failure_does_not_turn_successful_injection_into_an_error() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let (mut port, restored) = mock_port(events, 41, true);

        let result = execute_clipboard_paste(
            &mut port,
            "private text",
            10,
            || PasteDispatchOutcome {
                result: Ok(()),
                action_key_accepted: true,
            },
            |_| {},
        );

        assert!(result.is_ok());
        assert!(!restored.get());
    }

    #[test]
    fn send_failure_is_preserved_after_best_effort_restore() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let (mut port, restored) = mock_port(events.clone(), 41, false);
        let send_events = events.clone();
        let wait_events = events.clone();

        let result = execute_clipboard_paste(
            &mut port,
            "private text",
            10,
            || {
                send_events.borrow_mut().push(TransactionEvent::Send);
                PasteDispatchOutcome::not_dispatched(Err(TypexError::new(
                    ErrorCode::InjectionBlocked,
                    "mock SendInput failure",
                )))
            },
            |duration| {
                wait_events
                    .borrow_mut()
                    .push(TransactionEvent::Wait(duration.as_millis() as u64));
            },
        )
        .unwrap_err();

        assert_eq!(result.code, ErrorCode::InjectionBlocked);
        assert!(restored.get());
        assert_eq!(
            *events.borrow(),
            vec![
                TransactionEvent::Begin,
                TransactionEvent::Wait(10),
                TransactionEvent::Send,
                TransactionEvent::Restore,
            ]
        );
    }

    #[test]
    fn partial_ctrl_v_waits_only_after_the_action_key_was_accepted() {
        for accepted in 1..=3 {
            let events = Rc::new(RefCell::new(Vec::new()));
            let (mut port, restored) = mock_port(events.clone(), 41, false);
            let send_events = events.clone();
            let wait_events = events.clone();

            let error = execute_clipboard_paste(
                &mut port,
                "private text",
                10,
                || {
                    send_events.borrow_mut().push(TransactionEvent::Send);
                    let strokes = ctrl_v_strokes();
                    let mut call_count = 0;
                    let outcome = send_input_batch_outcome_with(&strokes, |cleanup| {
                        call_count += 1;
                        SendInputAttempt::from_accepted(if call_count == 1 {
                            accepted
                        } else {
                            cleanup.len()
                        })
                    });
                    PasteDispatchOutcome {
                        action_key_accepted: outcome.accepted > 1,
                        result: outcome.result,
                    }
                },
                |duration| {
                    wait_events
                        .borrow_mut()
                        .push(TransactionEvent::Wait(duration.as_millis() as u64));
                },
            )
            .unwrap_err();

            assert_eq!(error.code, ErrorCode::InjectionBlocked);
            assert!(
                error
                    .message
                    .contains(&format!("accepted {accepted}/4 events"))
            );
            assert!(restored.get());
            let mut expected = vec![
                TransactionEvent::Begin,
                TransactionEvent::Wait(10),
                TransactionEvent::Send,
            ];
            if accepted >= 2 {
                expected.push(TransactionEvent::Wait(RESTORE_DELAY_MS));
            }
            expected.push(TransactionEvent::Restore);
            assert_eq!(*events.borrow(), expected, "accepted prefix {accepted}");
        }
    }

    #[test]
    fn ctrl_v_has_balanced_down_and_up_events() {
        let strokes = ctrl_v_strokes();
        assert_eq!(strokes.len(), 4);
        assert_eq!(strokes[0], KeyStroke::virtual_key(VK_CONTROL_CODE, false));
        assert_eq!(strokes[1], KeyStroke::virtual_key(VK_V_CODE, false));
        assert_eq!(strokes[2], KeyStroke::virtual_key(VK_V_CODE, true));
        assert_eq!(strokes[3], KeyStroke::virtual_key(VK_CONTROL_CODE, true));
    }

    #[test]
    fn ctrl_v_partial_acceptance_releases_only_keys_still_down() {
        let strokes = ctrl_v_strokes();
        let ctrl_up = KeyStroke::virtual_key(VK_CONTROL_CODE, true);
        let v_up = KeyStroke::virtual_key(VK_V_CODE, true);
        let expected = [
            vec![],
            vec![ctrl_up],
            vec![v_up, ctrl_up],
            vec![ctrl_up],
            vec![],
        ];

        for (accepted, expected_cleanup) in expected.into_iter().enumerate() {
            assert_eq!(
                cleanup_keyups_for_accepted_prefix(&strokes, accepted),
                expected_cleanup,
                "accepted prefix length {accepted}"
            );
        }
    }

    #[test]
    fn cleanup_releases_multiple_keys_in_reverse_press_order() {
        let strokes = vec![
            KeyStroke::virtual_key(VK_CONTROL_CODE, false),
            KeyStroke::virtual_key(VK_V_CODE, false),
            KeyStroke::virtual_key(VK_C_CODE, false),
        ];

        assert_eq!(
            cleanup_keyups_for_accepted_prefix(&strokes, strokes.len()),
            vec![
                KeyStroke::virtual_key(VK_C_CODE, true),
                KeyStroke::virtual_key(VK_V_CODE, true),
                KeyStroke::virtual_key(VK_CONTROL_CODE, true),
            ]
        );
    }

    #[test]
    fn cleanup_tracks_repeated_key_state_without_duplicate_keyups() {
        let strokes = vec![
            KeyStroke::virtual_key(VK_V_CODE, false),
            KeyStroke::virtual_key(VK_V_CODE, false),
            KeyStroke::virtual_key(VK_V_CODE, true),
            KeyStroke::virtual_key(VK_V_CODE, false),
        ];

        assert!(cleanup_keyups_for_accepted_prefix(&strokes, 3).is_empty());
        assert_eq!(
            cleanup_keyups_for_accepted_prefix(&strokes, 4),
            vec![KeyStroke::virtual_key(VK_V_CODE, true)]
        );
    }

    #[test]
    fn cleanup_distinguishes_virtual_keys_from_unicode_code_units() {
        let strokes = vec![
            KeyStroke::virtual_key(VK_C_CODE, false),
            KeyStroke::unicode(VK_C_CODE, false),
        ];

        assert_eq!(
            cleanup_keyups_for_accepted_prefix(&strokes, strokes.len()),
            vec![
                KeyStroke::unicode(VK_C_CODE, true),
                KeyStroke::virtual_key(VK_C_CODE, true),
            ]
        );
    }

    #[test]
    fn partial_send_calls_keyup_cleanup_after_the_original_batch() {
        let strokes = ctrl_v_strokes();
        let mut calls = Vec::<Vec<KeyStroke>>::new();

        let error = send_input_batch_with(&strokes, |batch| {
            calls.push(batch.to_vec());
            SendInputAttempt::from_accepted(if calls.len() == 1 { 2 } else { batch.len() })
        })
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::InjectionBlocked);
        assert_eq!(
            calls,
            vec![
                strokes,
                vec![
                    KeyStroke::virtual_key(VK_V_CODE, true),
                    KeyStroke::virtual_key(VK_CONTROL_CODE, true),
                ],
            ]
        );
    }

    #[test]
    fn cleanup_failure_preserves_the_original_partial_send_error() {
        let strokes = ctrl_v_strokes();
        let mut call_count = 0usize;

        let error = send_input_batch_with(&strokes, |_| {
            call_count += 1;
            SendInputAttempt::from_accepted(if call_count == 1 { 1 } else { 0 })
        })
        .unwrap_err();

        assert_eq!(call_count, 2);
        assert_eq!(error.code, ErrorCode::InjectionBlocked);
        assert!(error.message.contains("accepted 1/4 events"));
    }

    #[test]
    fn zero_or_full_send_does_not_issue_a_cleanup_batch() {
        let strokes = ctrl_v_strokes();
        for accepted in [0, strokes.len()] {
            let mut call_count = 0usize;
            let result = send_input_batch_with(&strokes, |_| {
                call_count += 1;
                SendInputAttempt::from_accepted(accepted)
            });

            assert_eq!(call_count, 1);
            assert_eq!(result.is_ok(), accepted == strokes.len());
        }
    }

    #[test]
    fn unicode_bmp_has_down_and_up() {
        let strokes = unicode_strokes("中");
        assert_eq!(
            strokes,
            vec![
                KeyStroke::unicode('中' as u16, false),
                KeyStroke::unicode('中' as u16, true),
            ]
        );
    }

    #[test]
    fn unicode_emoji_sends_each_surrogate_with_keyup() {
        let units: Vec<u16> = "😀".encode_utf16().collect();
        let strokes = unicode_strokes("😀");
        assert_eq!(strokes.len(), 4);
        assert_eq!(strokes[0], KeyStroke::unicode(units[0], false));
        assert_eq!(strokes[1], KeyStroke::unicode(units[0], true));
        assert_eq!(strokes[2], KeyStroke::unicode(units[1], false));
        assert_eq!(strokes[3], KeyStroke::unicode(units[1], true));
    }

    #[test]
    fn emoji_partial_acceptance_releases_the_accepted_surrogate_only() {
        let units: Vec<u16> = "😀".encode_utf16().collect();
        let strokes = unicode_strokes("😀");

        assert!(cleanup_keyups_for_accepted_prefix(&strokes, 0).is_empty());
        assert_eq!(
            cleanup_keyups_for_accepted_prefix(&strokes, 1),
            vec![KeyStroke::unicode(units[0], true)]
        );
        assert!(cleanup_keyups_for_accepted_prefix(&strokes, 2).is_empty());
        assert_eq!(
            cleanup_keyups_for_accepted_prefix(&strokes, 3),
            vec![KeyStroke::unicode(units[1], true)]
        );
        assert!(cleanup_keyups_for_accepted_prefix(&strokes, strokes.len()).is_empty());
    }

    #[test]
    fn newlines_use_return_and_crlf_is_normalized() {
        let strokes = unicode_strokes("a\r\nb");
        assert_eq!(strokes.len(), 6);
        assert_eq!(strokes[2], KeyStroke::virtual_key(VK_RETURN_CODE, false));
        assert_eq!(strokes[3], KeyStroke::virtual_key(VK_RETURN_CODE, true));
    }

    #[test]
    #[ignore = "temporarily writes and restores the real Windows clipboard"]
    fn native_clipboard_owner_allows_write_and_restore() {
        let mut port = NativeClipboardPort::new().expect("create clipboard owner window");
        let (snapshot, sequence) = port
            .begin("typex-clipboard-owner-contract")
            .expect("message-only owner must allow SetClipboardData");
        let disposition = port
            .restore_if_unchanged(snapshot, sequence)
            .expect("original clipboard must be restorable");
        assert_eq!(disposition, RestoreDisposition::Restored);
    }
}
