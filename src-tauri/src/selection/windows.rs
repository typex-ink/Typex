//! Windows UI Automation selection reader running on a dedicated COM worker.

use super::{SelectionBounds, SelectionReader};
use crate::error::{ErrorCode, Result, TypexError};
use crate::platform::focus::FocusTarget;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
};
use std::time::Duration;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor};

const UIA_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
enum UiaSelection {
    Unsupported,
    Supported {
        text: Option<String>,
        bounds_px: Option<SelectionBounds>,
    },
}

struct Request {
    response: Sender<Result<UiaSelection>>,
    target: Option<FocusTarget>,
}

pub struct WindowsSelectionReader {
    tx: SyncSender<Request>,
    uia_healthy: AtomicBool,
    // Outer Option means whether read() populated the cache; inner None means no bounds available.
    cached_bounds: Mutex<Option<Option<SelectionBounds>>>,
}

impl WindowsSelectionReader {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("typex-uia".into())
            .spawn(move || worker(rx, ready_tx))
            .expect("spawn UI Automation worker");
        let healthy = matches!(ready_rx.recv_timeout(Duration::from_secs(2)), Ok(Ok(())));
        Self {
            tx,
            uia_healthy: AtomicBool::new(healthy),
            cached_bounds: Mutex::new(None),
        }
    }

    fn query(&self, target: Option<&FocusTarget>) -> Result<UiaSelection> {
        query_via_worker(&self.tx, &self.uia_healthy, UIA_TIMEOUT, target)
    }
}

fn query_via_worker(
    tx: &SyncSender<Request>,
    uia_healthy: &AtomicBool,
    timeout: Duration,
    target: Option<&FocusTarget>,
) -> Result<UiaSelection> {
    if !uia_healthy.load(Ordering::Acquire) {
        return Err(TypexError::new(
            ErrorCode::Timeout,
            "UI Automation worker was disabled after a timeout",
        ));
    }
    let (response, rx) = mpsc::channel();
    match tx.try_send(Request {
        response,
        target: target.cloned(),
    }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            return Err(TypexError::new(
                ErrorCode::Timeout,
                "UI Automation worker is still processing a previous request",
            ));
        }
        Err(TrySendError::Disconnected(_)) => {
            return Err(TypexError::new(ErrorCode::Internal, "UIA worker stopped"));
        }
    }
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            uia_healthy.store(false, Ordering::Release);
            Err(TypexError::new(
                ErrorCode::Timeout,
                "UI Automation selection query timed out",
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(TypexError::new(
            ErrorCode::Internal,
            "UIA worker disconnected",
        )),
    }
}

impl Default for WindowsSelectionReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionReader for WindowsSelectionReader {
    fn read(&self) -> Result<Option<String>> {
        let target = FocusTarget::capture();
        self.read_targeted(target.as_ref())
    }

    fn read_targeted(&self, target: Option<&FocusTarget>) -> Result<Option<String>> {
        validate_target(target, "before selection read")?;
        let (result, bounds_px) = resolve_selection(self.query(target), || {
            crate::inject::windows::read_selected_text_to(target)
        });
        let bounds = bounds_px.map(physical_bounds_to_logical);
        *self.cached_bounds.lock().unwrap_or_else(|e| e.into_inner()) = Some(bounds);
        validate_target(target, "during selection read")?;
        result
    }

    fn read_bounds(&self) -> Result<Option<SelectionBounds>> {
        if let Some(bounds) = self
            .cached_bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            return Ok(bounds);
        }
        match self.query(None) {
            Ok(UiaSelection::Supported { bounds_px, .. }) => {
                Ok(bounds_px.map(physical_bounds_to_logical))
            }
            Ok(UiaSelection::Unsupported) => Ok(None),
            Err(error) => {
                tracing::debug!(code = ?error.code, "UIA bounds unavailable");
                Ok(None)
            }
        }
    }
}

fn resolve_selection(
    query: Result<UiaSelection>,
    fallback: impl FnOnce() -> Result<Option<String>>,
) -> (Result<Option<String>>, Option<SelectionBounds>) {
    match query {
        Ok(UiaSelection::Supported { text, bounds_px }) => (Ok(text), bounds_px),
        Ok(UiaSelection::Unsupported) => (
            fallback().map_err(|fallback| {
                TypexError::new(
                    ErrorCode::Internal,
                    format!(
                        "UI Automation TextPattern unsupported; clipboard fallback failed: {}",
                        fallback.message
                    ),
                )
            }),
            None,
        ),
        Err(error) => {
            tracing::debug!(
                code = ?error.code,
                classification = %error.message,
                "UIA selection failed; using clipboard fallback"
            );
            (
                fallback().map_err(|fallback| {
                    TypexError::new(
                        error.code,
                        format!(
                            "{}; clipboard fallback failed: {}",
                            error.message, fallback.message
                        ),
                    )
                }),
                None,
            )
        }
    }
}

fn worker(rx: Receiver<Request>, ready: SyncSender<std::result::Result<(), String>>) {
    // SAFETY: COM is initialized and uninitialized on this same dedicated thread.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() };
    if let Err(error) = initialized {
        let message = format!("CoInitializeEx failed: {error}");
        let _ = ready.send(Err(message.clone()));
        drain_with_error(rx, message);
        return;
    }

    // SAFETY: CUIAutomation is an in-process COM class; the interface remains on this thread.
    let automation: windows::core::Result<IUIAutomation> =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
    match automation {
        Ok(automation) => {
            let _ = ready.send(Ok(()));
            for request in rx {
                let result = if request
                    .target
                    .as_ref()
                    .is_some_and(|target| !target.is_current())
                {
                    Err(TypexError::new(
                        ErrorCode::NoFocus,
                        "foreground target changed before UI Automation query",
                    ))
                } else {
                    let result = query_selection(&automation, request.target.as_ref());
                    if request
                        .target
                        .as_ref()
                        .is_some_and(|target| !target.is_current())
                    {
                        Err(TypexError::new(
                            ErrorCode::NoFocus,
                            "foreground target changed during UI Automation query",
                        ))
                    } else {
                        result
                    }
                };
                let _ = request.response.send(result);
            }
        }
        Err(error) => {
            let message = format!("CUIAutomation unavailable: {error}");
            let _ = ready.send(Err(message.clone()));
            drain_with_error(rx, message);
        }
    }

    // SAFETY: Balances the successful CoInitializeEx above on this thread.
    unsafe { CoUninitialize() };
}

fn validate_target(target: Option<&FocusTarget>, phase: &'static str) -> Result<()> {
    if crate::platform::focus::captured_target_is_current(target) {
        Ok(())
    } else {
        Err(TypexError::new(
            ErrorCode::NoFocus,
            format!("foreground target changed {phase}"),
        ))
    }
}

fn drain_with_error(rx: Receiver<Request>, message: String) {
    for request in rx {
        let _ = request
            .response
            .send(Err(TypexError::new(ErrorCode::Internal, message.clone())));
    }
}

fn query_selection(
    automation: &IUIAutomation,
    target: Option<&FocusTarget>,
) -> Result<UiaSelection> {
    // SAFETY: All COM interfaces are created and consumed on the initialized worker thread.
    unsafe {
        let focused = automation
            .GetFocusedElement()
            .map_err(|e| TypexError::new(ErrorCode::NoFocus, format!("UIA focus: {e}")))?;
        if let Some(target) = target {
            let focused_pid = focused.CurrentProcessId().map_err(|e| {
                TypexError::new(ErrorCode::NoFocus, format!("UIA focus process: {e}"))
            })?;
            if focused_pid < 0 || focused_pid as u32 != target.windows_process_id() {
                return Err(TypexError::new(
                    ErrorCode::NoFocus,
                    "UI Automation focus belongs to a different process",
                ));
            }
            if let Ok(native) = focused.CurrentNativeWindowHandle()
                && !native.is_invalid()
            {
                let root = GetAncestor(native, GA_ROOT);
                if !root.is_invalid() && root.0 as isize != target.windows_window_id() {
                    return Err(TypexError::new(
                        ErrorCode::NoFocus,
                        "UI Automation focus belongs to a different root window",
                    ));
                }
            }
        }
        let pattern: IUIAutomationTextPattern = match focused.GetCurrentPatternAs(UIA_TextPatternId)
        {
            Ok(pattern) => pattern,
            Err(_) => return Ok(UiaSelection::Unsupported),
        };
        let ranges = pattern
            .GetSelection()
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("UIA selection: {e}")))?;
        let count = ranges
            .Length()
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("UIA ranges: {e}")))?;
        if count <= 0 {
            return Ok(UiaSelection::Supported {
                text: None,
                bounds_px: None,
            });
        }

        let mut range_data = Vec::new();
        for index in 0..count {
            let range = ranges.GetElement(index).map_err(|e| {
                TypexError::new(ErrorCode::Internal, format!("UIA range {index}: {e}"))
            })?;
            let text = range.GetText(-1).map_err(|e| {
                TypexError::new(ErrorCode::Internal, format!("UIA text {index}: {e}"))
            })?;
            let text = String::try_from(text).map_err(|e| {
                TypexError::new(ErrorCode::Internal, format!("UIA UTF-16 text: {e}"))
            })?;
            let rects = range
                .GetBoundingRectangles()
                .map(|array| read_rectangles(array))
                .unwrap_or_default();
            range_data.push((text, rects));
        }
        Ok(merge_selection_ranges(range_data))
    }
}

fn merge_selection_ranges(
    ranges: impl IntoIterator<Item = (String, Vec<SelectionBounds>)>,
) -> UiaSelection {
    let mut texts = Vec::new();
    let mut rects = Vec::new();
    for (text, range_rects) in ranges {
        if !text.is_empty() {
            texts.push(text);
        }
        rects.extend(range_rects);
    }
    UiaSelection::Supported {
        text: (!texts.is_empty()).then(|| texts.join("\n")),
        bounds_px: union_rectangles(&rects),
    }
}

/// Copies and destroys UIA's SAFEARRAY of doubles: left, top, width, height.
unsafe fn read_rectangles(
    array: *mut windows::Win32::System::Com::SAFEARRAY,
) -> Vec<SelectionBounds> {
    if array.is_null() {
        return Vec::new();
    }
    let values = (|| -> windows::core::Result<Vec<f64>> {
        // SAFETY: The SAFEARRAY comes from UIA and remains owned here until SafeArrayDestroy.
        let lower = unsafe { SafeArrayGetLBound(array, 1)? };
        // SAFETY: Same valid one-dimensional SAFEARRAY as above.
        let upper = unsafe { SafeArrayGetUBound(array, 1)? };
        if upper < lower {
            return Ok(Vec::new());
        }
        let len = (upper - lower + 1) as usize;
        let mut data = std::ptr::null_mut();
        // SAFETY: Access is paired with SafeArrayUnaccessData before destruction.
        unsafe { SafeArrayAccessData(array, &mut data)? };
        let copied = if data.is_null() {
            Vec::new()
        } else {
            // SAFETY: UIA documents a VT_R8 array with exactly len elements.
            unsafe { std::slice::from_raw_parts(data.cast::<f64>(), len) }.to_vec()
        };
        // SAFETY: Balances SafeArrayAccessData.
        unsafe { SafeArrayUnaccessData(array)? };
        Ok(copied)
    })()
    .unwrap_or_default();
    // SAFETY: UIA transfers ownership of the returned SAFEARRAY to the caller.
    let _ = unsafe { SafeArrayDestroy(array) };

    values
        .chunks_exact(4)
        .filter_map(|v| {
            (v[2] > 0.0 && v[3] > 0.0).then_some(SelectionBounds {
                x: v[0],
                y: v[1],
                width: v[2],
                height: v[3],
            })
        })
        .collect()
}

fn union_rectangles(rects: &[SelectionBounds]) -> Option<SelectionBounds> {
    let first = rects.first()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x + first.width;
    let mut bottom = first.y + first.height;
    for rect in &rects[1..] {
        left = left.min(rect.x);
        top = top.min(rect.y);
        right = right.max(rect.x + rect.width);
        bottom = bottom.max(rect.y + rect.height);
    }
    Some(SelectionBounds {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn physical_bounds_to_logical(bounds: SelectionBounds) -> SelectionBounds {
    use crate::platform::windows::{
        PhysicalScreenRect, foreground_monitor_work_area, physical_rect_to_logical_screen,
    };

    let Some(monitor) = foreground_monitor_work_area() else {
        return bounds;
    };
    let rect = PhysicalScreenRect {
        left: bounds.x.floor() as i32,
        top: bounds.y.floor() as i32,
        right: (bounds.x + bounds.width).ceil() as i32,
        bottom: (bounds.y + bounds.height).ceil() as i32,
    };
    let logical = physical_rect_to_logical_screen(rect, monitor);
    SelectionBounds {
        x: logical.x,
        y: logical.y,
        width: logical.width,
        height: logical.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn selection_bounds(x: f64, y: f64, width: f64, height: f64) -> SelectionBounds {
        SelectionBounds {
            x,
            y,
            width,
            height,
        }
    }

    fn worker_response(result: Result<UiaSelection>) -> Result<UiaSelection> {
        let (tx, rx): (SyncSender<Request>, Receiver<Request>) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let request = rx.recv().expect("receive mock UIA request");
            let _ = request.response.send(result);
        });
        let healthy = AtomicBool::new(true);
        let response = query_via_worker(&tx, &healthy, Duration::from_secs(1), None);
        worker.join().expect("join mock UIA worker");
        response
    }

    #[test]
    fn supported_empty_selection_does_not_use_clipboard_fallback() {
        let fallback_called = Cell::new(false);
        let (text, bounds) = resolve_selection(
            Ok(UiaSelection::Supported {
                text: None,
                bounds_px: None,
            }),
            || {
                fallback_called.set(true);
                Ok(Some("unexpected".into()))
            },
        );

        assert_eq!(text.unwrap(), None);
        assert_eq!(bounds, None);
        assert!(!fallback_called.get());
    }

    #[test]
    fn supported_nonempty_selection_keeps_text_and_bounds() {
        let expected_bounds = selection_bounds(-120.0, 50.0, 60.0, 20.0);
        let (text, bounds) = resolve_selection(
            Ok(UiaSelection::Supported {
                text: Some("selected".into()),
                bounds_px: Some(expected_bounds),
            }),
            || panic!("supported UIA selection must not use clipboard fallback"),
        );

        assert_eq!(text.unwrap().as_deref(), Some("selected"));
        assert_eq!(bounds, Some(expected_bounds));
    }

    #[test]
    fn unsupported_selection_uses_clipboard_fallback() {
        let fallback_called = Cell::new(false);
        let (text, bounds) = resolve_selection(Ok(UiaSelection::Unsupported), || {
            fallback_called.set(true);
            Ok(Some("fallback".into()))
        });

        assert_eq!(text.unwrap().as_deref(), Some("fallback"));
        assert_eq!(bounds, None);
        assert!(fallback_called.get());
    }

    #[test]
    fn internal_uia_error_uses_fallback_and_preserves_error_if_it_fails() {
        let (text, bounds) = resolve_selection(
            Err(TypexError::new(ErrorCode::Internal, "mock COM failure")),
            || {
                Err(TypexError::new(
                    ErrorCode::Internal,
                    "mock fallback failure",
                ))
            },
        );

        let error = text.unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("mock COM failure"));
        assert!(error.message.contains("mock fallback failure"));
        assert_eq!(bounds, None);
    }

    #[test]
    fn internal_uia_error_accepts_successful_clipboard_fallback() {
        let (text, bounds) = resolve_selection(
            Err(TypexError::new(ErrorCode::Internal, "mock COM failure")),
            || Ok(Some("fallback".into())),
        );

        assert_eq!(text.unwrap().as_deref(), Some("fallback"));
        assert_eq!(bounds, None);
    }

    #[test]
    fn worker_returns_supported_and_internal_results() {
        let supported = worker_response(Ok(UiaSelection::Supported {
            text: Some("worker".into()),
            bounds_px: None,
        }))
        .unwrap();
        assert!(matches!(
            supported,
            UiaSelection::Supported {
                text: Some(text),
                bounds_px: None
            } if text == "worker"
        ));

        let error = worker_response(Err(TypexError::new(
            ErrorCode::Internal,
            "mock COM failure",
        )))
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
    }

    #[test]
    fn worker_timeout_opens_circuit_breaker() {
        let (tx, rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let request = rx.recv().expect("receive mock UIA request");
            std::thread::sleep(Duration::from_millis(50));
            drop(request);
        });
        let healthy = AtomicBool::new(true);

        let error = query_via_worker(&tx, &healthy, Duration::from_millis(5), None).unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        assert!(!healthy.load(Ordering::Acquire));

        let disabled = query_via_worker(&tx, &healthy, Duration::from_secs(1), None).unwrap_err();
        assert_eq!(disabled.code, ErrorCode::Timeout);
        assert!(disabled.message.contains("disabled"));
        worker.join().expect("join stalled mock UIA worker");
    }

    #[test]
    fn worker_reports_full_and_disconnected_queues() {
        let healthy = AtomicBool::new(true);
        let (full_tx, _full_rx) = mpsc::sync_channel(1);
        let (response, _response_rx) = mpsc::channel();
        assert!(
            full_tx
                .try_send(Request {
                    response,
                    target: None,
                })
                .is_ok()
        );
        let full = query_via_worker(&full_tx, &healthy, Duration::from_secs(1), None).unwrap_err();
        assert_eq!(full.code, ErrorCode::Timeout);
        assert!(full.message.contains("previous request"));

        let (stopped_tx, stopped_rx) = mpsc::sync_channel(1);
        drop(stopped_rx);
        let stopped =
            query_via_worker(&stopped_tx, &healthy, Duration::from_secs(1), None).unwrap_err();
        assert_eq!(stopped.code, ErrorCode::Internal);
        assert!(stopped.message.contains("stopped"));
    }

    #[test]
    fn worker_reports_disconnected_response_channel() {
        let (tx, rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let request = rx.recv().expect("receive mock UIA request");
            drop(request);
        });
        let healthy = AtomicBool::new(true);

        let error = query_via_worker(&tx, &healthy, Duration::from_secs(1), None).unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("disconnected"));
        worker.join().expect("join disconnected mock UIA worker");
    }

    #[test]
    fn merges_multiple_uia_range_texts_and_bounds() {
        let merged = merge_selection_ranges([
            (
                "first".into(),
                vec![selection_bounds(-120.0, 50.0, 20.0, 10.0)],
            ),
            (String::new(), vec![]),
            (
                "second".into(),
                vec![selection_bounds(-90.0, 60.0, 30.0, 10.0)],
            ),
        ]);

        match merged {
            UiaSelection::Supported { text, bounds_px } => {
                assert_eq!(text.as_deref(), Some("first\nsecond"));
                assert_eq!(bounds_px, Some(selection_bounds(-120.0, 50.0, 60.0, 20.0)));
            }
            UiaSelection::Unsupported => panic!("range merge must remain supported"),
        }
    }

    #[test]
    fn unions_multiple_uia_rectangles() {
        let bounds = union_rectangles(&[
            selection_bounds(-120.0, 50.0, 20.0, 10.0),
            selection_bounds(-90.0, 60.0, 30.0, 10.0),
        ])
        .unwrap();
        assert_eq!(bounds.x, -120.0);
        assert_eq!(bounds.y, 50.0);
        assert_eq!(bounds.width, 60.0);
        assert_eq!(bounds.height, 20.0);
    }

    #[test]
    fn empty_rectangles_have_no_bounds() {
        assert_eq!(union_rectangles(&[]), None);
    }
}
