//! Windows low-level keyboard hook backend.
//!
//! The Win32 callback stays deliberately small: normalize one raw event, run the
//! shared [`HotkeyDetector`], enqueue semantic events, and immediately return.
//! Hook installation and removal live on a dedicated message-loop thread.

use super::{HotkeyConfig, HotkeyDetector, HotkeyEvent};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::thread::JoinHandle;
use std::time::Instant;
use tokio::sync::{mpsc, watch};
use windows::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, KillTimer,
    LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED, MSG, PM_NOREMOVE, PeekMessageW,
    PostQuitMessage, PostThreadMessageW, SetTimer, SetWindowsHookExW, TranslateMessage,
    USER_TIMER_MINIMUM, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
};

// The synthetic Ctrl and physical Right Alt are indistinguishable from an intentional Right Alt
// gesture until the next key arrives. Default right-side modifiers also need a short silent window
// so Ctrl+C / AltGr typing can yield before recording, HUD, or chime effects start. Only the
// semantic TriggerDown is delayed; release timing still uses the original raw timestamp.
const ALTGR_PAIR_WAIT_MS: u64 = 10;
const MODIFIER_CONFIRMATION_MS: u64 = 75;

/// Current state of the native hook, suitable for the diagnostics surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsHookHealth {
    Starting,
    Healthy,
    Failed(WindowsHookError),
    /// The message loop ended without an application-requested shutdown.
    Stopped,
    /// Expected application teardown; never presented as a runtime failure.
    Shutdown,
}

impl WindowsHookHealth {
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }

    pub fn is_unexpected_terminal(&self) -> bool {
        matches!(self, Self::Failed(_) | Self::Stopped)
    }
}

/// Failures are classified without logging key data or foreground-window data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WindowsHookError {
    #[error("a Windows keyboard hook is already active")]
    AlreadyRunning,
    #[error("failed to spawn the Windows keyboard hook thread: {message}")]
    ThreadSpawn { message: String },
    #[error("the Windows keyboard hook initialization channel closed")]
    InitializationChannelClosed,
    #[error("SetWindowsHookExW failed with OS error {code}")]
    Install { code: i32 },
    #[error("GetMessageW failed with OS error {code}")]
    MessageLoop { code: i32 },
    #[error("UnhookWindowsHookEx failed with OS error {code}")]
    Uninstall { code: i32 },
    #[error("PostThreadMessageW(WM_QUIT) failed with OS error {code}")]
    ShutdownPost { code: i32 },
    #[error("the Windows keyboard hook callback panicked")]
    CallbackPanicked,
    #[error("the Windows keyboard hook thread panicked")]
    ThreadPanicked,
    #[error("the hotkey event consumer closed")]
    EventChannelClosed,
}

/// Owns the hook thread. Dropping this handle posts `WM_QUIT`, waits for the
/// message loop, and therefore removes the hook before process teardown.
pub struct WindowsHotkeyHandle {
    thread_id: u32,
    thread: Mutex<Option<JoinHandle<()>>>,
    health_rx: watch::Receiver<WindowsHookHealth>,
    shutdown_requested: Arc<AtomicBool>,
}

impl WindowsHotkeyHandle {
    pub fn health(&self) -> WindowsHookHealth {
        self.health_rx.borrow().clone()
    }

    pub fn subscribe_health(&self) -> watch::Receiver<WindowsHookHealth> {
        self.health_rx.clone()
    }

    pub fn shutdown(&self) -> Result<(), WindowsHookError> {
        if self.shutdown_requested.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        // SAFETY: thread_id comes from the initialized hook thread, whose
        // message queue is created before spawn() returns.
        let post_result =
            unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Err(error) = post_result {
            let terminal = matches!(
                self.health(),
                WindowsHookHealth::Stopped
                    | WindowsHookHealth::Shutdown
                    | WindowsHookHealth::Failed(_)
            );
            if !terminal {
                self.shutdown_requested.store(false, Ordering::Release);
                return Err(WindowsHookError::ShutdownPost {
                    code: error.code().0,
                });
            }
        }

        let mut thread = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(join) = thread.take() {
            // Joining the current thread would deadlock. This branch is only
            // relevant if ownership is ever moved into the hook callback.
            if unsafe { GetCurrentThreadId() } != self.thread_id && join.join().is_err() {
                return Err(WindowsHookError::ThreadPanicked);
            }
        }
        Ok(())
    }
}

impl Drop for WindowsHotkeyHandle {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Start the dedicated `WH_KEYBOARD_LL` message loop.
///
/// The returned handle must be retained for as long as `events` is consumed.
/// Hook installation is acknowledged synchronously, so `Ok` always means the
/// backend reached [`WindowsHookHealth::Healthy`].
pub fn spawn(
    initial: HotkeyConfig,
    config_rx: watch::Receiver<HotkeyConfig>,
    paused_rx: watch::Receiver<bool>,
) -> Result<(mpsc::UnboundedReceiver<HotkeyEvent>, WindowsHotkeyHandle), WindowsHookError> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (health_tx, health_rx) = watch::channel(WindowsHookHealth::Starting);
    let (init_tx, init_rx) = std_mpsc::sync_channel(1);
    let panic_health = health_tx.clone();
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let thread_shutdown_requested = shutdown_requested.clone();
    let panic_shutdown_requested = shutdown_requested.clone();

    let thread = std::thread::Builder::new()
        .name("typex-hotkey-win32".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_hook_thread(
                    initial,
                    config_rx,
                    paused_rx,
                    event_tx,
                    health_tx,
                    init_tx,
                    thread_shutdown_requested,
                );
            }));
            if result.is_err() {
                let health = if panic_shutdown_requested.load(Ordering::Acquire) {
                    WindowsHookHealth::Shutdown
                } else {
                    WindowsHookHealth::Failed(WindowsHookError::ThreadPanicked)
                };
                panic_health.send_replace(health);
            }
        })
        .map_err(|error| WindowsHookError::ThreadSpawn {
            message: error.to_string(),
        })?;

    let thread_id = match init_rx.recv() {
        Ok(Ok(thread_id)) => thread_id,
        Ok(Err(error)) => {
            let _ = thread.join();
            return Err(error);
        }
        Err(_) => {
            let _ = thread.join();
            return Err(WindowsHookError::InitializationChannelClosed);
        }
    };

    Ok((
        event_rx,
        WindowsHotkeyHandle {
            thread_id,
            thread: Mutex::new(Some(thread)),
            health_rx,
            shutdown_requested,
        },
    ))
}

type Initialization = Result<u32, WindowsHookError>;

fn run_hook_thread(
    initial: HotkeyConfig,
    config_rx: watch::Receiver<HotkeyConfig>,
    paused_rx: watch::Receiver<bool>,
    event_tx: mpsc::UnboundedSender<HotkeyEvent>,
    health_tx: watch::Sender<WindowsHookHealth>,
    init_tx: std_mpsc::SyncSender<Initialization>,
    shutdown_requested: Arc<AtomicBool>,
) {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MSG::default();

    // A thread message queue is created lazily. Create it before publishing the
    // thread id so WindowsHotkeyHandle::shutdown can always post WM_QUIT.
    unsafe {
        let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
    }

    let mut state = Box::new(HookThreadState::new(
        initial,
        config_rx,
        paused_rx,
        event_tx,
        health_tx.clone(),
    ));
    let state_ptr = ptr::from_mut(state.as_mut());
    if ACTIVE_HOOK_STATE
        .compare_exchange(
            ptr::null_mut(),
            state_ptr,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        let error = WindowsHookError::AlreadyRunning;
        health_tx.send_replace(WindowsHookHealth::Failed(error.clone()));
        let _ = init_tx.send(Err(error));
        return;
    }

    // SAFETY: low_level_keyboard_proc has the required system ABI and remains
    // linked for the lifetime of the process. The hook is removed below.
    let hook = match unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0)
    } {
        Ok(hook) => hook,
        Err(error) => {
            ACTIVE_HOOK_STATE.store(ptr::null_mut(), Ordering::Release);
            let error = WindowsHookError::Install {
                code: error.code().0,
            };
            health_tx.send_replace(WindowsHookHealth::Failed(error.clone()));
            let _ = init_tx.send(Err(error));
            return;
        }
    };
    let mut registration = HookRegistration::new(hook, state_ptr);

    health_tx.send_replace(WindowsHookHealth::Healthy);
    if init_tx.send(Ok(thread_id)).is_err() {
        return;
    }

    let loop_error = loop {
        // SAFETY: message is valid writable storage and this dedicated thread
        // owns the queue for the entire loop.
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            break Some(WindowsHookError::MessageLoop {
                code: unsafe { GetLastError().0 as i32 },
            });
        }
        if result.0 == 0 {
            break None;
        }
        if message.message == WM_TIMER && state.pending_timer_id == Some(message.wParam.0) {
            state.flush_pending();
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };

    let uninstall_error = registration.uninstall().err();
    let final_health = settle_hook_health(
        health_tx.borrow().clone(),
        loop_error.or(uninstall_error),
        shutdown_requested.load(Ordering::Acquire),
    );
    health_tx.send_replace(final_health);
}

fn settle_hook_health(
    current: WindowsHookHealth,
    error: Option<WindowsHookError>,
    shutdown_requested: bool,
) -> WindowsHookHealth {
    match current {
        failed @ WindowsHookHealth::Failed(_) => failed,
        _ => final_hook_health(error, shutdown_requested),
    }
}

fn final_hook_health(
    error: Option<WindowsHookError>,
    shutdown_requested: bool,
) -> WindowsHookHealth {
    error.map_or_else(
        || {
            if shutdown_requested {
                WindowsHookHealth::Shutdown
            } else {
                WindowsHookHealth::Stopped
            }
        },
        WindowsHookHealth::Failed,
    )
}

struct HookRegistration {
    hook: Option<HHOOK>,
    state_ptr: *mut HookThreadState,
}

impl HookRegistration {
    fn new(hook: HHOOK, state_ptr: *mut HookThreadState) -> Self {
        Self {
            hook: Some(hook),
            state_ptr,
        }
    }

    fn uninstall(&mut self) -> Result<(), WindowsHookError> {
        ACTIVE_HOOK_STATE
            .compare_exchange(
                self.state_ptr,
                ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok();
        let Some(hook) = self.hook.take() else {
            return Ok(());
        };
        unsafe { UnhookWindowsHookEx(hook) }.map_err(|error| WindowsHookError::Uninstall {
            code: error.code().0,
        })
    }
}

impl Drop for HookRegistration {
    fn drop(&mut self) {
        let _ = self.uninstall();
    }
}

static ACTIVE_HOOK_STATE: AtomicPtr<HookThreadState> = AtomicPtr::new(ptr::null_mut());

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code != HC_ACTION as i32 || lparam.0 == 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let Some(transition) = transition_from_message(wparam.0 as u32) else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };
    let state_ptr = ACTIVE_HOOK_STATE.load(Ordering::Acquire);
    let Some(state) = (unsafe { state_ptr.as_mut() }) else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };
    let keyboard = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let raw = RawKeyboardEvent {
        vk_code: keyboard.vkCode,
        scan_code: keyboard.scanCode,
        flags: keyboard.flags.0,
        transition,
        t_ms: state.epoch.elapsed().as_millis() as u64,
        native_time: keyboard.time,
    };

    let decision = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| state.process(raw)));
    match decision {
        Ok(true) => LRESULT(1),
        Ok(false) => unsafe { CallNextHookEx(None, code, wparam, lparam) },
        Err(_) => {
            state.fail_terminal(WindowsHookError::CallbackPanicked);
            ACTIVE_HOOK_STATE
                .compare_exchange(
                    state_ptr,
                    ptr::null_mut(),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .ok();
            unsafe { CallNextHookEx(None, code, wparam, lparam) }
        }
    }
}

fn transition_from_message(message: u32) -> Option<KeyTransition> {
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyTransition::Down),
        WM_KEYUP | WM_SYSKEYUP => Some(KeyTransition::Up),
        _ => None,
    }
}

struct HookThreadState {
    adapter: WindowsEventAdapter,
    config_rx: watch::Receiver<HotkeyConfig>,
    paused_rx: watch::Receiver<bool>,
    paused: bool,
    event_tx: mpsc::UnboundedSender<HotkeyEvent>,
    health_tx: watch::Sender<WindowsHookHealth>,
    accepting_events: AtomicBool,
    epoch: Instant,
    pending_timer_id: Option<usize>,
}

impl HookThreadState {
    fn new(
        initial: HotkeyConfig,
        config_rx: watch::Receiver<HotkeyConfig>,
        paused_rx: watch::Receiver<bool>,
        event_tx: mpsc::UnboundedSender<HotkeyEvent>,
        health_tx: watch::Sender<WindowsHookHealth>,
    ) -> Self {
        let paused = *paused_rx.borrow();
        Self {
            adapter: WindowsEventAdapter::new(initial),
            config_rx,
            paused_rx,
            paused,
            event_tx,
            health_tx,
            accepting_events: AtomicBool::new(true),
            epoch: Instant::now(),
            pending_timer_id: None,
        }
    }

    /// Returns whether this raw event is the one confirmed Typex RAlt keyup
    /// that must be swallowed to avoid activating the foreground menu bar.
    fn process(&mut self, raw: RawKeyboardEvent) -> bool {
        if !self.refresh_runtime_state() || self.paused {
            return false;
        }

        let decision = self.adapter.process(raw);
        let swallow = decision.swallow;
        if !self.emit_events(decision.events) {
            return false;
        }
        self.reschedule_pending_timer();
        swallow
    }

    fn refresh_runtime_state(&mut self) -> bool {
        if !self.accepting_events.load(Ordering::Acquire) {
            return false;
        }
        if matches!(self.config_rx.has_changed(), Ok(true)) {
            let config = self.config_rx.borrow_and_update().clone();
            let decision = self
                .adapter
                .set_config(config, self.epoch.elapsed().as_millis() as u64);
            if !self.emit_events(decision.events) {
                return false;
            }
        }

        let paused = *self.paused_rx.borrow_and_update();
        if paused != self.paused {
            self.paused = paused;
            let events = self.adapter.reset();
            if !self.emit_events(events) {
                return false;
            }
            self.reschedule_pending_timer();
        }
        self.accepting_events.load(Ordering::Acquire)
    }

    fn flush_pending(&mut self) {
        if !self.refresh_runtime_state() || self.paused {
            return;
        }
        let decision = self
            .adapter
            .flush_due(self.epoch.elapsed().as_millis() as u64);
        let _ = self.emit_events(decision.events);
        self.reschedule_pending_timer();
    }

    fn emit_events(&mut self, events: Vec<HotkeyEvent>) -> bool {
        if !self.accepting_events.load(Ordering::Acquire) {
            return false;
        }
        for event in events {
            if self.event_tx.send(event).is_err() {
                self.fail_terminal(WindowsHookError::EventChannelClosed);
                return false;
            }
        }
        true
    }

    fn transition_terminal(&mut self, error: WindowsHookError) -> bool {
        if self.accepting_events.swap(false, Ordering::AcqRel) {
            for event in self.adapter.reset() {
                let _ = self.event_tx.send(event);
            }
            self.health_tx
                .send_replace(WindowsHookHealth::Failed(error));
            true
        } else {
            false
        }
    }

    fn fail_terminal(&mut self, error: WindowsHookError) {
        if self.transition_terminal(error) {
            unsafe { PostQuitMessage(1) };
        }
    }

    fn reschedule_pending_timer(&mut self) {
        if let Some(timer_id) = self.pending_timer_id.take() {
            unsafe {
                let _ = KillTimer(None, timer_id);
            }
        }
        if !self.accepting_events.load(Ordering::Acquire) {
            return;
        }
        let Some(deadline) = self.adapter.next_deadline() else {
            return;
        };
        let now = self.epoch.elapsed().as_millis() as u64;
        let delay = deadline
            .saturating_sub(now)
            .clamp(u64::from(USER_TIMER_MINIMUM), u64::from(u32::MAX)) as u32;
        let timer_id = unsafe { SetTimer(None, 0, delay, None) };
        if timer_id != 0 {
            self.pending_timer_id = Some(timer_id);
        }
    }
}

impl Drop for HookThreadState {
    fn drop(&mut self) {
        if let Some(timer_id) = self.pending_timer_id.take() {
            unsafe {
                let _ = KillTimer(None, timer_id);
            }
        }
        for event in self.adapter.reset() {
            let _ = self.event_tx.send(event);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyTransition {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy)]
struct RawKeyboardEvent {
    vk_code: u32,
    scan_code: u32,
    flags: u32,
    transition: KeyTransition,
    t_ms: u64,
    native_time: u32,
}

impl RawKeyboardEvent {
    fn is_down(self) -> bool {
        self.transition == KeyTransition::Down
    }

    fn is_injected(self) -> bool {
        self.flags & (LLKHF_INJECTED.0 | LLKHF_LOWER_IL_INJECTED.0) != 0
    }

    fn is_extended(self) -> bool {
        self.flags & LLKHF_EXTENDED.0 != 0
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct HookDecision {
    events: Vec<HotkeyEvent>,
    swallow: bool,
}

impl HookDecision {
    fn extend(&mut self, other: Self) {
        self.events.extend(other.events);
        self.swallow |= other.swallow;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightAltDisposition {
    None,
    PendingAltGr,
    AltGrBypass,
    ChordCandidate,
    TypexGesture,
    Yielded,
}

#[derive(Debug, Clone, Copy)]
struct PendingRawEvent {
    raw: RawKeyboardEvent,
    deadline_ms: u64,
}

#[derive(Debug, Clone)]
struct PendingSemanticEvent {
    event: HotkeyEvent,
    token: u64,
    deadline_ms: u64,
}

/// Pure adapter around HotkeyDetector. It owns Windows-only concerns that must
/// be proven without installing a global hook: injection filtering, AltGr
/// disambiguation, and the narrowly-scoped RAlt keyup interception decision.
struct WindowsEventAdapter {
    config: HotkeyConfig,
    detector: HotkeyDetector,
    pending_left_ctrl: Option<PendingRawEvent>,
    pending_altgr: Option<PendingRawEvent>,
    prestarted_altgr_candidate: Option<u64>,
    pending_modifier: Option<PendingSemanticEvent>,
    synthetic_left_ctrl_down: bool,
    right_alt: RightAltDisposition,
    swallow_next_right_alt_up: bool,
    next_candidate_token: u64,
}

impl WindowsEventAdapter {
    fn new(config: HotkeyConfig) -> Self {
        let config = config.normalized();
        Self {
            detector: HotkeyDetector::new(config.clone()),
            config,
            pending_left_ctrl: None,
            pending_altgr: None,
            prestarted_altgr_candidate: None,
            pending_modifier: None,
            synthetic_left_ctrl_down: false,
            right_alt: RightAltDisposition::None,
            swallow_next_right_alt_up: false,
            next_candidate_token: 1,
        }
    }

    fn set_config(&mut self, config: HotkeyConfig, t_ms: u64) -> HookDecision {
        let config = config.normalized();
        if self.config == config {
            return HookDecision::default();
        }
        if self.config.same_chords(&config) {
            let _ = self.detector.set_config(config.clone(), t_ms);
            self.config = config;
            return HookDecision::default();
        }
        if self.right_alt == RightAltDisposition::TypexGesture {
            self.swallow_next_right_alt_up = true;
        }
        let pending_trigger = self.pending_modifier.take();
        let prestarted_altgr = self.prestarted_altgr_candidate.take();
        let mut events = self.detector.set_config(config.clone(), t_ms);
        if let Some(pending) = pending_trigger {
            events.retain(|event| !matches!(event, HotkeyEvent::TriggerUp { .. }));
            events.insert(
                0,
                HotkeyEvent::CaptureCandidateCancelled {
                    token: pending.token,
                },
            );
        }
        if let Some(token) = prestarted_altgr {
            events.insert(0, HotkeyEvent::CaptureCandidateCancelled { token });
        }
        self.config = config.clone();
        self.pending_left_ctrl = None;
        self.pending_altgr = None;
        self.synthetic_left_ctrl_down = false;
        self.right_alt = RightAltDisposition::None;
        HookDecision {
            events,
            swallow: false,
        }
    }

    fn reset(&mut self) -> Vec<HotkeyEvent> {
        let events = self
            .pending_modifier
            .take()
            .map(|pending| HotkeyEvent::CaptureCandidateCancelled {
                token: pending.token,
            })
            .into_iter()
            .chain(
                self.prestarted_altgr_candidate
                    .take()
                    .map(|token| HotkeyEvent::CaptureCandidateCancelled { token }),
            )
            .collect();
        self.detector = HotkeyDetector::new(self.config.clone());
        self.pending_left_ctrl = None;
        self.pending_altgr = None;
        self.synthetic_left_ctrl_down = false;
        self.right_alt = RightAltDisposition::None;
        self.swallow_next_right_alt_up = false;
        events
    }

    fn process(&mut self, raw: RawKeyboardEvent) -> HookDecision {
        // Do not mutate physical-key state for SendInput events: an injected
        // keyup must not release a real key currently held by the user.
        if raw.is_injected() {
            return HookDecision::default();
        }

        let key = decode_key_id(raw);
        if key == "AltRight"
            && raw.is_down()
            && self
                .pending_left_ctrl
                .is_some_and(|pending| is_synthetic_altgr_pair(pending.raw, raw))
        {
            self.pending_left_ctrl = None;
            self.synthetic_left_ctrl_down = true;
            self.pending_altgr = Some(PendingRawEvent {
                raw,
                deadline_ms: raw.t_ms.saturating_add(MODIFIER_CONFIRMATION_MS),
            });
            self.right_alt = RightAltDisposition::PendingAltGr;
            if self.pending_modifier.is_none()
                && !self.detector.has_active_gesture()
                && (self.config.dictation.as_slice() == ["AltRight"]
                    || self.config.assistant.as_slice() == ["AltRight"])
            {
                let token = self.allocate_candidate_token();
                self.prestarted_altgr_candidate = Some(token);
                tracing::debug!(raw_event_to_candidate_ms = 0_u64);
                return HookDecision {
                    events: vec![HotkeyEvent::CaptureCandidateStarted { token }],
                    swallow: false,
                };
            }
            return HookDecision::default();
        }

        // A raw key arriving exactly on the confirmation boundary still belongs to the silent
        // window. Timer delivery at the same timestamp confirms normally via flush_due().
        let mut decision = self.flush_before_input(raw.t_ms);
        if key == "ControlLeft" {
            if raw.is_down() {
                self.pending_left_ctrl = Some(PendingRawEvent {
                    raw,
                    deadline_ms: raw.t_ms.saturating_add(ALTGR_PAIR_WAIT_MS),
                });
            } else if self.synthetic_left_ctrl_down {
                self.synthetic_left_ctrl_down = false;
            } else {
                decision.extend(self.flush_left_ctrl());
                decision.extend(self.process_key(raw, &key));
            }
            return decision;
        }

        if key == "AltRight" && raw.is_down() {
            if let Some(pending) = self.pending_left_ctrl.take() {
                decision.extend(self.process_key(pending.raw, "ControlLeft"));
            }
            decision.extend(self.process_right_alt(raw));
            return decision;
        }

        decision.extend(self.flush_left_ctrl());

        if self.pending_altgr.is_some() {
            if (key == "AltRight" && !raw.is_down()) || self.config.is_trigger_key(&key) {
                decision.extend(self.activate_pending_altgr());
            } else if raw.is_down() && (key != "Escape" || self.config.esc_cancels) {
                self.pending_altgr = None;
                self.right_alt = RightAltDisposition::AltGrBypass;
                if let Some(token) = self.prestarted_altgr_candidate.take() {
                    decision
                        .events
                        .push(HotkeyEvent::CaptureCandidateCancelled { token });
                }
                decision.extend(self.process_key(raw, &key));
                return decision;
            }
        }

        if key == "AltRight" {
            decision.extend(self.process_right_alt(raw));
        } else {
            decision.extend(self.process_key(raw, &key));
        }
        decision
    }

    fn process_key(&mut self, raw: RawKeyboardEvent, key: &str) -> HookDecision {
        let events = self.detector.on_key(key, raw.is_down(), raw.t_ms);
        if events.contains(&HotkeyEvent::Yielded)
            && matches!(
                self.right_alt,
                RightAltDisposition::ChordCandidate | RightAltDisposition::TypexGesture
            )
        {
            self.right_alt = RightAltDisposition::Yielded;
        } else if self.right_alt == RightAltDisposition::ChordCandidate
            && events.iter().any(|event| {
                matches!(
                    event,
                    HotkeyEvent::TriggerDown { .. } | HotkeyEvent::ModeUpgraded { .. }
                )
            })
        {
            self.right_alt = RightAltDisposition::TypexGesture;
        }
        HookDecision {
            events: self.sequence_modifier_events(raw, key, events),
            swallow: false,
        }
    }

    fn sequence_modifier_events(
        &mut self,
        raw: RawKeyboardEvent,
        key: &str,
        mut events: Vec<HotkeyEvent>,
    ) -> Vec<HotkeyEvent> {
        let deferred_trigger = if raw.is_down() && matches!(key, "ControlRight" | "AltRight") {
            events
                .iter()
                .position(|event| matches!(event, HotkeyEvent::TriggerDown { .. }))
                .map(|position| events.remove(position))
        } else {
            None
        };

        if self.pending_modifier.is_some() {
            if self.config.esc_cancels
                && events
                    .iter()
                    .any(|event| matches!(event, HotkeyEvent::EscPressed))
            {
                let pending = self.pending_modifier.take().expect("checked above");
                events.insert(
                    0,
                    HotkeyEvent::CaptureCandidateCancelled {
                        token: pending.token,
                    },
                );
                if self.right_alt == RightAltDisposition::ChordCandidate {
                    self.right_alt = RightAltDisposition::Yielded;
                }
            } else if events.iter().any(|event| {
                matches!(
                    event,
                    HotkeyEvent::ModeUpgraded { .. } | HotkeyEvent::TriggerUp { .. }
                )
            }) {
                let pending = self.pending_modifier.take().expect("checked above");
                events.insert(0, pending.event);
            } else if let Some(yielded) = events
                .iter()
                .position(|event| matches!(event, HotkeyEvent::Yielded))
            {
                // The candidate never reached the orchestrator, so its matching yield must also
                // stay silent. HotkeyDetector retains yielded state until the modifier is released.
                let pending = self.pending_modifier.take().expect("checked above");
                events[yielded] = HotkeyEvent::CaptureCandidateCancelled {
                    token: pending.token,
                };
            }
        }

        if let Some(HotkeyEvent::TriggerDown { mode }) = deferred_trigger {
            // This also covers stale-release recovery, where the detector emits Yielded followed
            // by a fresh TriggerDown. The old session is cancelled now; the new one is confirmed
            // independently so Ctrl+C cannot bypass the silent window on the recovery path.
            let prestarted = self.prestarted_altgr_candidate.take();
            let token = prestarted.unwrap_or_else(|| self.allocate_candidate_token());
            self.pending_modifier = Some(PendingSemanticEvent {
                event: HotkeyEvent::CaptureCandidatePromoted { token, mode },
                token,
                deadline_ms: raw.t_ms.saturating_add(MODIFIER_CONFIRMATION_MS),
            });
            if prestarted.is_none() {
                tracing::debug!(raw_event_to_candidate_ms = 0_u64);
                events.push(HotkeyEvent::CaptureCandidateStarted { token });
            }
        }
        events
    }

    fn allocate_candidate_token(&mut self) -> u64 {
        let token = self.next_candidate_token;
        self.next_candidate_token = self.next_candidate_token.wrapping_add(1).max(1);
        token
    }

    fn flush_pending_modifier(&mut self) -> HookDecision {
        self.pending_modifier
            .take()
            .map_or_else(HookDecision::default, |pending| {
                if matches!(&pending.event, HotkeyEvent::CaptureCandidatePromoted { .. })
                    && matches!(
                        self.right_alt,
                        RightAltDisposition::ChordCandidate | RightAltDisposition::PendingAltGr
                    )
                {
                    self.right_alt = RightAltDisposition::TypexGesture;
                }
                HookDecision {
                    events: vec![pending.event],
                    swallow: false,
                }
            })
    }

    fn flush_left_ctrl(&mut self) -> HookDecision {
        self.pending_left_ctrl
            .take()
            .map_or_else(HookDecision::default, |pending| {
                self.process_key(pending.raw, "ControlLeft")
            })
    }

    fn activate_pending_altgr(&mut self) -> HookDecision {
        let Some(pending) = self.pending_altgr.take() else {
            return HookDecision::default();
        };
        self.right_alt = RightAltDisposition::None;
        self.process_right_alt(pending.raw)
    }

    fn flush_before_input(&mut self, now_ms: u64) -> HookDecision {
        let mut decision = HookDecision::default();
        if self
            .pending_left_ctrl
            .is_some_and(|pending| pending.deadline_ms < now_ms)
        {
            decision.extend(self.flush_left_ctrl());
        }
        if self
            .pending_altgr
            .is_some_and(|pending| pending.deadline_ms < now_ms)
        {
            decision.extend(self.activate_pending_altgr());
        }
        if self
            .pending_modifier
            .as_ref()
            .is_some_and(|pending| pending.deadline_ms < now_ms)
        {
            decision.extend(self.flush_pending_modifier());
        }
        decision
    }

    fn flush_due(&mut self, now_ms: u64) -> HookDecision {
        let mut decision = HookDecision::default();
        if self
            .pending_left_ctrl
            .is_some_and(|pending| pending.deadline_ms <= now_ms)
        {
            decision.extend(self.flush_left_ctrl());
        }
        if self
            .pending_altgr
            .is_some_and(|pending| pending.deadline_ms <= now_ms)
        {
            decision.extend(self.activate_pending_altgr());
        }
        if self
            .pending_modifier
            .as_ref()
            .is_some_and(|pending| pending.deadline_ms <= now_ms)
        {
            decision.extend(self.flush_pending_modifier());
        }
        decision
    }

    fn next_deadline(&self) -> Option<u64> {
        self.pending_left_ctrl
            .map(|pending| pending.deadline_ms)
            .into_iter()
            .chain(self.pending_altgr.map(|pending| pending.deadline_ms))
            .chain(
                self.pending_modifier
                    .as_ref()
                    .map(|pending| pending.deadline_ms),
            )
            .min()
    }

    fn process_right_alt(&mut self, raw: RawKeyboardEvent) -> HookDecision {
        if !raw.is_down() && std::mem::take(&mut self.swallow_next_right_alt_up) {
            self.right_alt = RightAltDisposition::None;
            return HookDecision {
                events: Vec::new(),
                swallow: true,
            };
        }
        if self.right_alt == RightAltDisposition::AltGrBypass {
            if !raw.is_down() {
                self.right_alt = RightAltDisposition::None;
            }
            return HookDecision::default();
        }

        let was_typex = self.right_alt == RightAltDisposition::TypexGesture;
        let detector_events = self.detector.on_key("AltRight", raw.is_down(), raw.t_ms);
        let yielded = detector_events
            .iter()
            .any(|event| matches!(event, HotkeyEvent::Yielded));
        if raw.is_down()
            && self.config.is_trigger_key("AltRight")
            && self.right_alt == RightAltDisposition::None
        {
            self.right_alt = RightAltDisposition::ChordCandidate;
        }
        let events = self.sequence_modifier_events(raw, "AltRight", detector_events);
        let confirmed_now = events.iter().any(|event| {
            matches!(
                event,
                HotkeyEvent::TriggerDown { .. }
                    | HotkeyEvent::ModeUpgraded { .. }
                    | HotkeyEvent::CaptureCandidatePromoted { .. }
            )
        });
        if yielded
            || events
                .iter()
                .any(|event| matches!(event, HotkeyEvent::CaptureCandidateCancelled { .. }))
        {
            self.right_alt = RightAltDisposition::Yielded;
        } else if confirmed_now {
            self.right_alt = RightAltDisposition::TypexGesture;
        }
        let swallow = !raw.is_down() && (was_typex || confirmed_now);
        if !raw.is_down() {
            self.right_alt = RightAltDisposition::None;
        }
        HookDecision {
            events: self.sequence_modifier_events(raw, "AltRight", events),
            swallow,
        }
    }
}

fn is_synthetic_altgr_pair(left_ctrl: RawKeyboardEvent, right_alt: RawKeyboardEvent) -> bool {
    left_ctrl.is_down()
        && right_alt.is_down()
        && right_alt.is_extended()
        && left_ctrl.native_time == right_alt.native_time
}

// Virtual-key values are stable Win32 ABI constants. Keeping normalization in
// this pure function also lets fixtures exercise generic VK_CONTROL/VK_MENU +
// LLKHF_EXTENDED, not only the already-sided VK_RCONTROL/VK_RMENU values.
fn scan_code_key_id(raw: RawKeyboardEvent) -> Option<&'static str> {
    let extended = raw.is_extended();
    Some(match (raw.scan_code, extended) {
        (0x01, false) => "Escape",
        (0x02, false) => "Digit1",
        (0x03, false) => "Digit2",
        (0x04, false) => "Digit3",
        (0x05, false) => "Digit4",
        (0x06, false) => "Digit5",
        (0x07, false) => "Digit6",
        (0x08, false) => "Digit7",
        (0x09, false) => "Digit8",
        (0x0A, false) => "Digit9",
        (0x0B, false) => "Digit0",
        (0x0C, false) => "Minus",
        (0x0D, false) => "Equal",
        (0x0E, false) => "Backspace",
        (0x0F, false) => "Tab",
        (0x10, false) => "KeyQ",
        (0x11, false) => "KeyW",
        (0x12, false) => "KeyE",
        (0x13, false) => "KeyR",
        (0x14, false) => "KeyT",
        (0x15, false) => "KeyY",
        (0x16, false) => "KeyU",
        (0x17, false) => "KeyI",
        (0x18, false) => "KeyO",
        (0x19, false) => "KeyP",
        (0x1A, false) => "BracketLeft",
        (0x1B, false) => "BracketRight",
        (0x1C, false) => "Enter",
        (0x1C, true) => "NumpadEnter",
        (0x1D, false) => "ControlLeft",
        (0x1D, true) => "ControlRight",
        (0x1E, false) => "KeyA",
        (0x1F, false) => "KeyS",
        (0x20, false) => "KeyD",
        (0x21, false) => "KeyF",
        (0x22, false) => "KeyG",
        (0x23, false) => "KeyH",
        (0x24, false) => "KeyJ",
        (0x25, false) => "KeyK",
        (0x26, false) => "KeyL",
        (0x27, false) => "Semicolon",
        (0x28, false) => "Quote",
        (0x29, false) => "Backquote",
        (0x2A, false) => "ShiftLeft",
        (0x2B, false) => "Backslash",
        (0x2C, false) => "KeyZ",
        (0x2D, false) => "KeyX",
        (0x2E, false) => "KeyC",
        (0x2F, false) => "KeyV",
        (0x30, false) => "KeyB",
        (0x31, false) => "KeyN",
        (0x32, false) => "KeyM",
        (0x33, false) => "Comma",
        (0x34, false) => "Period",
        (0x35, false) => "Slash",
        (0x35, true) => "NumpadDivide",
        (0x36, false) => "ShiftRight",
        (0x37, false) => "NumpadMultiply",
        (0x37, true) if raw.vk_code == 0x2C => "PrintScreen",
        (0x38, false) => "AltLeft",
        (0x38, true) => "AltRight",
        (0x39, false) => "Space",
        (0x3A, false) => "CapsLock",
        (0x3B, false) => "F1",
        (0x3C, false) => "F2",
        (0x3D, false) => "F3",
        (0x3E, false) => "F4",
        (0x3F, false) => "F5",
        (0x40, false) => "F6",
        (0x41, false) => "F7",
        (0x42, false) => "F8",
        (0x43, false) => "F9",
        (0x44, false) => "F10",
        (0x45, _) if raw.vk_code == 0x13 => "Pause",
        (0x45, false) => "NumLock",
        (0x46, false) => "ScrollLock",
        (0x47, false) => "Numpad7",
        (0x47, true) => "Home",
        (0x48, false) => "Numpad8",
        (0x48, true) => "ArrowUp",
        (0x49, false) => "Numpad9",
        (0x49, true) => "PageUp",
        (0x4A, false) => "NumpadSubtract",
        (0x4B, false) => "Numpad4",
        (0x4B, true) => "ArrowLeft",
        (0x4C, false) => "Numpad5",
        (0x4D, false) => "Numpad6",
        (0x4D, true) => "ArrowRight",
        (0x4E, false) => "NumpadAdd",
        (0x4F, false) => "Numpad1",
        (0x4F, true) => "End",
        (0x50, false) => "Numpad2",
        (0x50, true) => "ArrowDown",
        (0x51, false) => "Numpad3",
        (0x51, true) => "PageDown",
        (0x52, false) => "Numpad0",
        (0x52, true) => "Insert",
        (0x53, false) => "NumpadDecimal",
        (0x53, true) => "Delete",
        (0x56, false) => "IntlBackslash",
        (0x57, false) => "F11",
        (0x58, false) => "F12",
        (0x5B, true) => "MetaLeft",
        (0x5C, true) => "MetaRight",
        (0x5D, true) => "Menu",
        (0x64, _) => "F13",
        (0x65, _) => "F14",
        (0x66, _) => "F15",
        (0x67, _) => "F16",
        (0x68, _) => "F17",
        (0x69, _) => "F18",
        (0x6A, _) => "F19",
        _ => return None,
    })
}

fn decode_key_id(raw: RawKeyboardEvent) -> String {
    if let Some(id) = scan_code_key_id(raw) {
        return id.to_string();
    }
    let id = match raw.vk_code {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x10 => {
            if raw.scan_code == 0x36 {
                "ShiftRight"
            } else {
                "ShiftLeft"
            }
        }
        0x11 => {
            if raw.is_extended() {
                "ControlRight"
            } else {
                "ControlLeft"
            }
        }
        0x12 => {
            if raw.is_extended() {
                "AltRight"
            } else {
                "AltLeft"
            }
        }
        0x13 => "Pause",
        0x14 => "CapsLock",
        0x1B => "Escape",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "ArrowLeft",
        0x26 => "ArrowUp",
        0x27 => "ArrowRight",
        0x28 => "ArrowDown",
        0x2C => "PrintScreen",
        0x2D => "Insert",
        0x2E => "Delete",
        0x30 => "Digit0",
        0x31 => "Digit1",
        0x32 => "Digit2",
        0x33 => "Digit3",
        0x34 => "Digit4",
        0x35 => "Digit5",
        0x36 => "Digit6",
        0x37 => "Digit7",
        0x38 => "Digit8",
        0x39 => "Digit9",
        0x41 => "KeyA",
        0x42 => "KeyB",
        0x43 => "KeyC",
        0x44 => "KeyD",
        0x45 => "KeyE",
        0x46 => "KeyF",
        0x47 => "KeyG",
        0x48 => "KeyH",
        0x49 => "KeyI",
        0x4A => "KeyJ",
        0x4B => "KeyK",
        0x4C => "KeyL",
        0x4D => "KeyM",
        0x4E => "KeyN",
        0x4F => "KeyO",
        0x50 => "KeyP",
        0x51 => "KeyQ",
        0x52 => "KeyR",
        0x53 => "KeyS",
        0x54 => "KeyT",
        0x55 => "KeyU",
        0x56 => "KeyV",
        0x57 => "KeyW",
        0x58 => "KeyX",
        0x59 => "KeyY",
        0x5A => "KeyZ",
        0x5B => "MetaLeft",
        0x5C => "MetaRight",
        0x5D => "Menu",
        0x60 => "Numpad0",
        0x61 => "Numpad1",
        0x62 => "Numpad2",
        0x63 => "Numpad3",
        0x64 => "Numpad4",
        0x65 => "Numpad5",
        0x66 => "Numpad6",
        0x67 => "Numpad7",
        0x68 => "Numpad8",
        0x69 => "Numpad9",
        0x6A => "NumpadMultiply",
        0x6B => "NumpadAdd",
        0x6D => "NumpadSubtract",
        0x6E => "NumpadDecimal",
        0x6F => "NumpadDivide",
        0x70 => "F1",
        0x71 => "F2",
        0x72 => "F3",
        0x73 => "F4",
        0x74 => "F5",
        0x75 => "F6",
        0x76 => "F7",
        0x77 => "F8",
        0x78 => "F9",
        0x79 => "F10",
        0x7A => "F11",
        0x7B => "F12",
        0x7C => "F13",
        0x7D => "F14",
        0x7E => "F15",
        0x7F => "F16",
        0x80 => "F17",
        0x81 => "F18",
        0x82 => "F19",
        0x90 => "NumLock",
        0x91 => "ScrollLock",
        0xA0 => "ShiftLeft",
        0xA1 => "ShiftRight",
        0xA2 => "ControlLeft",
        0xA3 => "ControlRight",
        0xA4 => "AltLeft",
        0xA5 => "AltRight",
        0xBA => "Semicolon",
        0xBB => "Equal",
        0xBC => "Comma",
        0xBD => "Minus",
        0xBE => "Period",
        0xBF => "Slash",
        0xC0 => "Backquote",
        0xDB => "BracketLeft",
        0xDC => "Backslash",
        0xDD => "BracketRight",
        0xDE => "Quote",
        0xE2 => "IntlBackslash",
        _ => return format!("Unknown({})", raw.vk_code),
    };
    id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionMode;

    const EXTENDED: u32 = 0x01;
    const INJECTED: u32 = 0x10;

    fn config() -> HotkeyConfig {
        HotkeyConfig {
            dictation: vec!["ControlRight".into()],
            assistant: vec!["AltRight".into()],
            translation: vec!["ControlRight".into(), "AltRight".into()],
            esc_cancels: true,
        }
    }

    fn candidate_started(token: u64) -> HotkeyEvent {
        HotkeyEvent::CaptureCandidateStarted { token }
    }

    fn candidate_promoted(token: u64, mode: SessionMode) -> HotkeyEvent {
        HotkeyEvent::CaptureCandidatePromoted { token, mode }
    }

    fn candidate_cancelled(token: u64) -> HotkeyEvent {
        HotkeyEvent::CaptureCandidateCancelled { token }
    }

    fn raw(vk_code: u32, down: bool, t_ms: u64) -> RawKeyboardEvent {
        RawKeyboardEvent {
            vk_code,
            scan_code: 0,
            flags: 0,
            transition: if down {
                KeyTransition::Down
            } else {
                KeyTransition::Up
            },
            t_ms,
            native_time: t_ms as u32,
        }
    }

    fn extended(vk_code: u32, down: bool, t_ms: u64) -> RawKeyboardEvent {
        RawKeyboardEvent {
            flags: EXTENDED,
            ..raw(vk_code, down, t_ms)
        }
    }

    fn scanned(vk_code: u32, scan_code: u32, is_extended: bool) -> RawKeyboardEvent {
        RawKeyboardEvent {
            vk_code,
            scan_code,
            flags: if is_extended { EXTENDED } else { 0 },
            transition: KeyTransition::Down,
            t_ms: 0,
            native_time: 0,
        }
    }

    fn native_time(mut raw: RawKeyboardEvent, value: u32) -> RawKeyboardEvent {
        raw.native_time = value;
        raw
    }

    fn synthetic_altgr_left_ctrl(down: bool, t_ms: u64, native: u32) -> RawKeyboardEvent {
        native_time(raw(0xA2, down, t_ms), native)
    }

    fn synthetic_altgr_right_alt(down: bool, t_ms: u64, native: u32) -> RawKeyboardEvent {
        native_time(extended(0xA5, down, t_ms), native)
    }

    #[test]
    fn decodes_sided_and_extended_modifier_variants() {
        assert_eq!(decode_key_id(raw(0xA3, true, 0)), "ControlRight");
        assert_eq!(decode_key_id(extended(0x11, true, 0)), "ControlRight");
        assert_eq!(decode_key_id(raw(0xA2, true, 0)), "ControlLeft");
        assert_eq!(decode_key_id(raw(0xA5, true, 0)), "AltRight");
        assert_eq!(decode_key_id(extended(0x12, true, 0)), "AltRight");
        assert_eq!(decode_key_id(raw(0x12, true, 0)), "AltLeft");

        let mut generic_right_shift = raw(0x10, true, 0);
        generic_right_shift.scan_code = 0x36;
        assert_eq!(decode_key_id(generic_right_shift), "ShiftRight");
    }

    #[test]
    fn scan_code_table_matches_the_stable_physical_key_contract() {
        let cases = [
            (0x0D, 0x1C, false, "Enter"),
            (0x31, 0x02, false, "Digit1"),
            (0x25, 0x4B, true, "ArrowLeft"),
            (0xA5, 0x38, true, "AltRight"),
            (0x5B, 0x5B, true, "MetaLeft"),
            (0x5D, 0x5D, true, "Menu"),
            (0, 0x64, false, "F13"),
            (0, 0x6A, false, "F19"),
            (0xBA, 0x27, false, "Semicolon"),
            (0xBE, 0x34, false, "Period"),
            (0xC0, 0x29, false, "Backquote"),
            (0xDB, 0x1A, false, "BracketLeft"),
            (0xDC, 0x2B, false, "Backslash"),
            (0x41, 0x1E, false, "KeyA"),
            (0x61, 0x4F, false, "Numpad1"),
        ];

        for (vk, scan, extended, expected) in cases {
            assert_eq!(
                decode_key_id(scanned(vk, scan, extended)),
                expected,
                "vk={vk:#x} scan={scan:#x}"
            );
        }
    }

    #[test]
    fn layout_dependent_virtual_key_does_not_override_physical_scan_position() {
        // The physical QWERTY Y position can report VK_Z on a QWERTZ layout.
        assert_eq!(decode_key_id(scanned(0x5A, 0x15, false)), "KeyY");
        assert_eq!(decode_key_id(scanned(0x59, 0x2C, false)), "KeyZ");
    }

    #[test]
    fn decodes_frontend_recordable_windows_fallback_keys() {
        assert_eq!(decode_key_id(raw(0x5D, true, 0)), "Menu");
        for (vk, expected) in (0x7C..=0x82).zip(["F13", "F14", "F15", "F16", "F17", "F18", "F19"]) {
            assert_eq!(decode_key_id(raw(vk, true, 0)), expected);
        }
    }

    #[test]
    fn recorded_f13_and_menu_bindings_reach_the_shared_detector() {
        let mut adapter = WindowsEventAdapter::new(HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["Menu".into()],
            translation: vec!["F13".into(), "Menu".into()],
            esc_cancels: true,
        });
        assert_eq!(
            adapter.process(raw(0x7C, true, 0)).events,
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(
            adapter.process(raw(0x5D, true, 10)).events,
            vec![HotkeyEvent::ModeUpgraded {
                mode: SessionMode::Translation
            }]
        );
    }

    #[test]
    fn config_update_ends_confirmed_single_chord_before_replacing_it() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(
            adapter.process(raw(0xA3, true, 100)).events,
            vec![candidate_started(1)]
        );
        assert_eq!(
            adapter.flush_due(100 + MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Dictation)]
        );

        let replacement = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        assert_eq!(
            adapter.set_config(replacement, 449).events,
            vec![HotkeyEvent::TriggerUp { held_ms: 349 }]
        );
        assert_eq!(
            adapter.process(raw(0xA3, false, 500)),
            HookDecision::default()
        );
    }

    #[test]
    fn config_update_ends_confirmed_multi_chord_without_duplicate_release() {
        let mut adapter = WindowsEventAdapter::new(HotkeyConfig {
            dictation: vec!["ControlRight".into(), "Digit1".into()],
            assistant: vec!["AltRight".into(), "KeyA".into()],
            translation: Vec::new(),
            esc_cancels: true,
        });
        assert_eq!(adapter.process(raw(0xA3, true, 0)), HookDecision::default());
        assert_eq!(
            adapter.process(raw(0x31, true, 10)).events,
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );

        let replacement = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        assert_eq!(
            adapter.set_config(replacement, 361).events,
            vec![HotkeyEvent::TriggerUp { held_ms: 351 }]
        );
        assert_eq!(
            adapter.process(raw(0x31, false, 400)),
            HookDecision::default()
        );
        assert_eq!(
            adapter.process(raw(0xA3, false, 410)),
            HookDecision::default()
        );
    }

    #[test]
    fn config_update_cancels_unflushed_modifier_candidate() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(
            adapter.process(raw(0xA3, true, 100)).events,
            vec![candidate_started(1)]
        );

        let replacement = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        assert_eq!(
            adapter.set_config(replacement, 140).events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(adapter.next_deadline(), None);
    }

    #[test]
    fn config_update_does_not_swallow_unconfirmed_right_alt_release() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(
            adapter.process(raw(0xA5, true, 100)).events,
            vec![candidate_started(1)]
        );
        let replacement = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        assert_eq!(
            adapter.set_config(replacement, 120).events,
            vec![candidate_cancelled(1)]
        );
        let release = adapter.process(raw(0xA5, false, 130));
        assert!(!release.swallow);
        assert!(release.events.is_empty());
    }

    #[test]
    fn identical_config_update_preserves_confirmed_and_pending_gestures() {
        let mut confirmed = WindowsEventAdapter::new(config());
        confirmed.process(raw(0xA3, true, 100));
        assert_eq!(
            confirmed.flush_due(100 + MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Dictation)]
        );
        assert_eq!(confirmed.set_config(config(), 200), HookDecision::default());
        assert_eq!(
            confirmed.process(raw(0xA3, false, 449)).events,
            vec![HotkeyEvent::TriggerUp { held_ms: 349 }]
        );

        let mut pending = WindowsEventAdapter::new(config());
        pending.process(raw(0xA3, true, 0));
        assert_eq!(pending.next_deadline(), Some(MODIFIER_CONFIRMATION_MS));
        assert_eq!(pending.set_config(config(), 10), HookDecision::default());
        assert_eq!(pending.next_deadline(), Some(MODIFIER_CONFIRMATION_MS));
        assert_eq!(
            pending.process(raw(0x43, true, 20)).events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(pending.next_deadline(), None);
    }

    #[test]
    fn config_update_swallows_the_confirmed_old_right_alt_release_once() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA5, true, 0));
        assert_eq!(
            adapter.flush_due(MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Assistant)]
        );

        let replacement = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        assert_eq!(
            adapter.set_config(replacement, 100).events,
            vec![HotkeyEvent::TriggerUp { held_ms: 100 }]
        );

        let first_release = adapter.process(raw(0xA5, false, 110));
        assert!(first_release.swallow);
        assert!(first_release.events.is_empty());
        assert_eq!(
            adapter.process(raw(0xA5, false, 120)),
            HookDecision::default()
        );
    }

    #[test]
    fn config_update_does_not_create_right_alt_tombstones_for_partial_yield_or_altgr() {
        let replacement = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };

        let mut partial = WindowsEventAdapter::new(HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["AltRight".into(), "KeyA".into()],
            translation: Vec::new(),
            esc_cancels: true,
        });
        partial.process(raw(0xA5, true, 0));
        assert_eq!(
            partial.set_config(replacement.clone(), 10),
            HookDecision::default()
        );
        assert!(!partial.process(raw(0xA5, false, 20)).swallow);

        let mut yielded = WindowsEventAdapter::new(config());
        yielded.process(raw(0xA5, true, 0));
        assert_eq!(
            yielded.process(raw(0x45, true, 20)).events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(
            yielded.set_config(replacement.clone(), 30),
            HookDecision::default()
        );
        assert!(!yielded.process(raw(0xA5, false, 40)).swallow);

        let mut altgr = WindowsEventAdapter::new(config());
        altgr.process(synthetic_altgr_left_ctrl(true, 0, 100));
        assert_eq!(
            altgr
                .process(synthetic_altgr_right_alt(true, 1, 100))
                .events,
            vec![candidate_started(1)]
        );
        assert_eq!(
            altgr.process(raw(0x45, true, 20)).events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(altgr.set_config(replacement, 30), HookDecision::default());
        assert!(
            !altgr
                .process(synthetic_altgr_right_alt(false, 40, 140))
                .swallow
        );
    }

    #[test]
    fn confirmed_multi_key_right_alt_chord_swallows_only_its_alt_keyup() {
        let mut adapter = WindowsEventAdapter::new(HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["AltRight".into(), "KeyA".into()],
            translation: Vec::new(),
            esc_cancels: true,
        });

        assert_eq!(adapter.process(raw(0xA5, true, 0)), HookDecision::default());
        assert_eq!(
            adapter.process(raw(0x41, true, 10)).events,
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Assistant
            }]
        );
        assert_eq!(adapter.process(raw(0x41, false, 40)).events, vec![]);
        let alt_up = adapter.process(raw(0xA5, false, 100));
        assert!(alt_up.swallow);
        assert_eq!(alt_up.events, vec![HotkeyEvent::TriggerUp { held_ms: 90 }]);
    }

    #[test]
    fn injected_events_never_reach_detector_or_change_physical_state() {
        let mut adapter = WindowsEventAdapter::new(config());
        let mut injected_down = raw(0xA3, true, 0);
        injected_down.flags = INJECTED;
        assert_eq!(adapter.process(injected_down), HookDecision::default());
        assert_eq!(
            adapter.process(raw(0xA3, false, 10)),
            HookDecision::default()
        );

        assert_eq!(
            adapter.process(raw(0xA5, true, 20)).events,
            vec![candidate_started(1)]
        );
        let mut injected_key = raw(0x45, true, 30);
        injected_key.flags = INJECTED;
        assert_eq!(adapter.process(injected_key), HookDecision::default());
        let release = adapter.process(raw(0xA5, false, 100));
        assert!(release.swallow);
        assert_eq!(
            release.events,
            vec![
                candidate_promoted(1, SessionMode::Assistant),
                HotkeyEvent::TriggerUp { held_ms: 80 }
            ]
        );
    }

    #[test]
    fn synthetic_altgr_typing_has_no_semantic_or_swallow_effects() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(
            adapter.process(synthetic_altgr_left_ctrl(true, 0, 100)),
            HookDecision::default()
        );
        assert_eq!(
            adapter
                .process(synthetic_altgr_right_alt(true, 1, 100))
                .events,
            vec![candidate_started(1)]
        );
        assert_eq!(
            adapter.process(raw(0x45, true, 30)).events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(
            adapter.process(raw(0x45, false, 40)),
            HookDecision::default()
        );
        let right_alt_up = adapter.process(synthetic_altgr_right_alt(false, 50, 150));
        assert_eq!(right_alt_up, HookDecision::default());
        assert!(!right_alt_up.swallow);
        assert_eq!(
            adapter.process(synthetic_altgr_left_ctrl(false, 51, 150)),
            HookDecision::default()
        );
    }

    #[test]
    fn standalone_synthetic_altgr_activates_assistant_after_grace_period() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(synthetic_altgr_left_ctrl(true, 100, 500));
        assert_eq!(
            adapter
                .process(synthetic_altgr_right_alt(true, 101, 500))
                .events,
            vec![candidate_started(1)]
        );

        assert_eq!(
            adapter.next_deadline(),
            Some(101 + MODIFIER_CONFIRMATION_MS)
        );
        assert_eq!(
            adapter.flush_due(101 + MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Assistant)]
        );

        let up = adapter.process(synthetic_altgr_right_alt(false, 500, 900));
        assert!(up.swallow);
        assert_eq!(up.events, vec![HotkeyEvent::TriggerUp { held_ms: 399 }]);
        assert_eq!(
            adapter.process(synthetic_altgr_left_ctrl(false, 501, 900)),
            HookDecision::default()
        );
    }

    #[test]
    fn quick_standalone_synthetic_altgr_preserves_toggle_timing() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(synthetic_altgr_left_ctrl(true, 100, 500));
        assert_eq!(
            adapter
                .process(synthetic_altgr_right_alt(true, 101, 500))
                .events,
            vec![candidate_started(1)]
        );

        let up = adapter.process(synthetic_altgr_right_alt(false, 140, 540));
        assert!(up.swallow);
        assert_eq!(
            up.events,
            vec![
                candidate_promoted(1, SessionMode::Assistant),
                HotkeyEvent::TriggerUp { held_ms: 39 }
            ]
        );
    }

    #[test]
    fn right_ctrl_plus_synthetic_altgr_upgrades_to_translation_without_yield() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(
            adapter.process(raw(0xA3, true, 0)).events,
            vec![candidate_started(1)]
        );
        assert_eq!(
            adapter.process(synthetic_altgr_left_ctrl(true, 10, 700)),
            HookDecision::default()
        );
        assert_eq!(
            adapter.process(synthetic_altgr_right_alt(true, 11, 700)),
            HookDecision::default()
        );
        assert_eq!(
            adapter.flush_due(11 + MODIFIER_CONFIRMATION_MS).events,
            vec![
                candidate_promoted(1, SessionMode::Dictation),
                HotkeyEvent::ModeUpgraded {
                    mode: SessionMode::Translation
                }
            ]
        );
    }

    #[test]
    fn synthetic_altgr_then_right_ctrl_upgrades_in_the_other_order() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(synthetic_altgr_left_ctrl(true, 0, 800));
        assert_eq!(
            adapter
                .process(synthetic_altgr_right_alt(true, 1, 800))
                .events,
            vec![candidate_started(1)]
        );

        assert_eq!(
            adapter.process(raw(0xA3, true, 20)).events,
            vec![
                candidate_promoted(1, SessionMode::Assistant),
                HotkeyEvent::ModeUpgraded {
                    mode: SessionMode::Translation
                }
            ]
        );
    }

    #[test]
    fn physical_left_ctrl_then_right_alt_is_not_misclassified_as_altgr() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(adapter.process(raw(0xA2, true, 0)), HookDecision::default());

        let right_alt = adapter.process(extended(0xA5, true, 20));
        assert_eq!(right_alt.events, vec![candidate_started(1)]);
        assert!(!right_alt.swallow);
        assert_eq!(
            adapter.flush_due(20 + MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Assistant)]
        );
    }

    #[test]
    fn configured_left_ctrl_fires_after_the_pair_detection_window() {
        let mut adapter = WindowsEventAdapter::new(HotkeyConfig {
            dictation: vec!["ControlLeft".into()],
            assistant: vec!["AltRight".into()],
            translation: vec!["ControlLeft".into(), "AltRight".into()],
            esc_cancels: true,
        });
        assert_eq!(
            adapter.process(raw(0xA2, true, 100)),
            HookDecision::default()
        );
        assert_eq!(
            adapter.flush_due(100 + ALTGR_PAIR_WAIT_MS).events,
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(
            adapter.process(raw(0xA2, false, 500)).events,
            vec![HotkeyEvent::TriggerUp { held_ms: 400 }]
        );
    }

    #[test]
    fn confirmed_right_alt_assistant_gesture_swallows_only_keyup() {
        let mut adapter = WindowsEventAdapter::new(config());
        let down = adapter.process(raw(0xA5, true, 100));
        assert!(!down.swallow);
        assert_eq!(down.events, vec![candidate_started(1)]);
        assert_eq!(
            adapter.flush_due(100 + MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Assistant)]
        );

        let up = adapter.process(raw(0xA5, false, 449));
        assert!(up.swallow);
        assert_eq!(up.events, vec![HotkeyEvent::TriggerUp { held_ms: 349 }]);
    }

    #[test]
    fn right_alt_keyup_is_not_swallowed_after_normal_key_yield() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA5, true, 0));
        assert_eq!(
            adapter.process(raw(0x45, true, 20)).events,
            vec![candidate_cancelled(1)]
        );
        adapter.process(raw(0x45, false, 40));
        let release = adapter.process(raw(0xA5, false, 50));
        assert!(!release.swallow);
        assert!(release.events.is_empty());
    }

    #[test]
    fn unconfigured_right_alt_is_never_swallowed() {
        let config = HotkeyConfig {
            dictation: vec!["ControlRight".into()],
            assistant: vec!["F13".into()],
            translation: vec!["ControlRight".into(), "F13".into()],
            esc_cancels: true,
        };
        let mut adapter = WindowsEventAdapter::new(config);
        assert_eq!(adapter.process(raw(0xA5, true, 0)), HookDecision::default());
        assert_eq!(
            adapter.process(raw(0xA5, false, 100)),
            HookDecision::default()
        );
    }

    #[test]
    fn right_ctrl_right_alt_upgrades_translation_in_either_order() {
        let mut first = WindowsEventAdapter::new(config());
        assert_eq!(
            first.process(raw(0xA3, true, 0)).events,
            vec![candidate_started(1)]
        );
        assert_eq!(
            first.process(raw(0xA5, true, 10)).events,
            vec![
                candidate_promoted(1, SessionMode::Dictation),
                HotkeyEvent::ModeUpgraded {
                    mode: SessionMode::Translation
                }
            ]
        );

        let mut second = WindowsEventAdapter::new(config());
        assert_eq!(
            second.process(raw(0xA5, true, 0)).events,
            vec![candidate_started(1)]
        );
        assert_eq!(
            second.process(raw(0xA3, true, 10)).events,
            vec![
                candidate_promoted(1, SessionMode::Assistant),
                HotkeyEvent::ModeUpgraded {
                    mode: SessionMode::Translation
                }
            ]
        );
    }

    #[test]
    fn right_ctrl_ctrl_c_inside_confirmation_window_has_no_semantic_events() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(
            adapter.process(raw(0xA3, true, 0)).events,
            vec![candidate_started(1)]
        );
        assert_eq!(adapter.next_deadline(), Some(MODIFIER_CONFIRMATION_MS));

        assert_eq!(
            adapter.process(raw(0x43, true, 20)).events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(adapter.next_deadline(), None);
        assert_eq!(
            adapter.process(raw(0x43, false, 40)),
            HookDecision::default()
        );
        assert_eq!(
            adapter.process(raw(0xA3, false, 50)),
            HookDecision::default()
        );
    }

    #[test]
    fn escape_cancels_pending_candidate_but_keeps_escape_semantics() {
        let mut adapter = WindowsEventAdapter::new(config());
        assert_eq!(
            adapter.process(raw(0xA3, true, 0)).events,
            vec![candidate_started(1)]
        );

        assert_eq!(
            adapter.process(raw(0x1B, true, 20)).events,
            vec![candidate_cancelled(1), HotkeyEvent::EscPressed]
        );
        assert_eq!(adapter.next_deadline(), None);
    }

    #[test]
    fn disabled_escape_keeps_pending_candidate_alive() {
        let mut disabled = config();
        disabled.esc_cancels = false;
        let mut adapter = WindowsEventAdapter::new(disabled);
        adapter.process(raw(0xA3, true, 0));

        assert_eq!(
            adapter.process(raw(0x1B, true, 20)).events,
            vec![HotkeyEvent::EscPressed]
        );
        assert_eq!(adapter.next_deadline(), Some(MODIFIER_CONFIRMATION_MS));
        assert_eq!(
            adapter.flush_due(MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Dictation)]
        );
    }

    #[test]
    fn escape_setting_update_does_not_cancel_pending_candidate() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA3, true, 0));
        let mut disabled = config();
        disabled.esc_cancels = false;

        assert_eq!(adapter.set_config(disabled, 10), HookDecision::default());
        assert_eq!(adapter.next_deadline(), Some(MODIFIER_CONFIRMATION_MS));
    }

    #[test]
    fn right_ctrl_ctrl_c_at_confirmation_boundary_still_yields_silently() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA3, true, 0));

        assert_eq!(
            adapter
                .process(raw(0x43, true, MODIFIER_CONFIRMATION_MS))
                .events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(adapter.next_deadline(), None);
    }

    #[test]
    fn physical_right_alt_typing_at_confirmation_boundary_has_no_side_effects() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA5, true, 0));

        assert_eq!(
            adapter
                .process(raw(0x45, true, MODIFIER_CONFIRMATION_MS))
                .events,
            vec![candidate_cancelled(1)]
        );
        let release = adapter.process(raw(0xA5, false, MODIFIER_CONFIRMATION_MS + 10));
        assert_eq!(release, HookDecision::default());
    }

    #[test]
    fn synthetic_altgr_typing_at_confirmation_boundary_has_no_side_effects() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(synthetic_altgr_left_ctrl(true, 0, 100));
        assert_eq!(
            adapter
                .process(synthetic_altgr_right_alt(true, 1, 100))
                .events,
            vec![candidate_started(1)]
        );

        assert_eq!(
            adapter
                .process(raw(0x45, true, 1 + MODIFIER_CONFIRMATION_MS))
                .events,
            vec![candidate_cancelled(1)]
        );
        assert_eq!(adapter.next_deadline(), None);
    }

    #[test]
    fn right_ctrl_alone_confirms_within_hud_latency_budget() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA3, true, 100));

        assert!(
            adapter
                .flush_due(100 + MODIFIER_CONFIRMATION_MS - 1)
                .events
                .is_empty()
        );
        assert!(std::hint::black_box(MODIFIER_CONFIRMATION_MS) <= 100);
        assert_eq!(
            adapter.flush_due(100 + MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Dictation)]
        );
    }

    #[test]
    fn quick_right_ctrl_release_preserves_original_toggle_timing() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA3, true, 100));

        assert_eq!(
            adapter.process(raw(0xA3, false, 140)).events,
            vec![
                candidate_promoted(1, SessionMode::Dictation),
                HotkeyEvent::TriggerUp { held_ms: 40 }
            ]
        );
    }

    #[test]
    fn stale_right_ctrl_recovery_reconfirms_the_fresh_trigger() {
        let mut adapter = WindowsEventAdapter::new(config());
        adapter.process(raw(0xA3, true, 0));
        assert_eq!(
            adapter.flush_due(MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(1, SessionMode::Dictation)]
        );

        assert_eq!(
            adapter.process(raw(0xA3, true, 500)).events,
            vec![HotkeyEvent::Yielded, candidate_started(2)]
        );
        assert_eq!(
            adapter.next_deadline(),
            Some(500 + MODIFIER_CONFIRMATION_MS)
        );
        assert_eq!(
            adapter.flush_due(500 + MODIFIER_CONFIRMATION_MS).events,
            vec![candidate_promoted(2, SessionMode::Dictation)]
        );
        assert_eq!(
            adapter.process(raw(0xA3, false, 900)).events,
            vec![HotkeyEvent::TriggerUp { held_ms: 400 }]
        );
    }

    #[test]
    fn expected_shutdown_has_a_distinct_non_error_terminal_health() {
        assert_eq!(final_hook_health(None, true), WindowsHookHealth::Shutdown);
        assert!(!final_hook_health(None, true).is_unexpected_terminal());
        assert_eq!(final_hook_health(None, false), WindowsHookHealth::Stopped);
        assert!(final_hook_health(None, false).is_unexpected_terminal());
    }

    #[test]
    fn shutdown_does_not_hide_a_real_unhook_failure() {
        let error = WindowsHookError::Uninstall { code: 5 };
        assert_eq!(
            final_hook_health(Some(error.clone()), true),
            WindowsHookHealth::Failed(error)
        );
    }

    #[test]
    fn callback_failure_is_not_overwritten_when_wm_quit_ends_the_loop() {
        let failed = WindowsHookHealth::Failed(WindowsHookError::CallbackPanicked);
        assert_eq!(settle_hook_health(failed.clone(), None, false), failed);
    }

    #[test]
    fn terminal_failure_atomically_blocks_all_followup_raw_events() {
        let terminal_config = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        let (_config_tx, config_rx) = watch::channel(terminal_config.clone());
        let (_paused_tx, paused_rx) = watch::channel(false);
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (health_tx, health_rx) = watch::channel(WindowsHookHealth::Healthy);
        let mut state =
            HookThreadState::new(terminal_config, config_rx, paused_rx, event_tx, health_tx);

        assert!(!state.process(raw(0x7C, true, 0)));
        assert_eq!(
            event_rx.try_recv().unwrap(),
            HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }
        );

        assert!(state.transition_terminal(WindowsHookError::CallbackPanicked));
        assert!(!state.transition_terminal(WindowsHookError::EventChannelClosed));
        assert_eq!(
            *health_rx.borrow(),
            WindowsHookHealth::Failed(WindowsHookError::CallbackPanicked)
        );

        assert!(!state.process(raw(0x7C, false, 100)));
        assert!(!state.process(raw(0x7D, true, 110)));
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn pause_transition_cancels_an_unconfirmed_candidate() {
        let (_config_tx, config_rx) = watch::channel(config());
        let (paused_tx, paused_rx) = watch::channel(false);
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (health_tx, _health_rx) = watch::channel(WindowsHookHealth::Healthy);
        let mut state = HookThreadState::new(config(), config_rx, paused_rx, event_tx, health_tx);

        assert!(!state.process(raw(0xA3, true, 0)));
        assert_eq!(event_rx.try_recv().unwrap(), candidate_started(1));
        paused_tx.send_replace(true);
        assert!(!state.process(raw(0x43, true, 10)));
        assert_eq!(event_rx.try_recv().unwrap(), candidate_cancelled(1));
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn hook_terminal_failure_cancels_an_unconfirmed_candidate() {
        let (_config_tx, config_rx) = watch::channel(config());
        let (_paused_tx, paused_rx) = watch::channel(false);
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (health_tx, _health_rx) = watch::channel(WindowsHookHealth::Healthy);
        let mut state = HookThreadState::new(config(), config_rx, paused_rx, event_tx, health_tx);

        assert!(!state.process(raw(0xA3, true, 0)));
        assert_eq!(event_rx.try_recv().unwrap(), candidate_started(1));
        assert!(state.transition_terminal(WindowsHookError::CallbackPanicked));
        assert_eq!(event_rx.try_recv().unwrap(), candidate_cancelled(1));
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn system_message_variants_decode_to_transitions() {
        assert_eq!(
            transition_from_message(WM_KEYDOWN),
            Some(KeyTransition::Down)
        );
        assert_eq!(
            transition_from_message(WM_SYSKEYDOWN),
            Some(KeyTransition::Down)
        );
        assert_eq!(transition_from_message(WM_KEYUP), Some(KeyTransition::Up));
        assert_eq!(
            transition_from_message(WM_SYSKEYUP),
            Some(KeyTransition::Up)
        );
        assert_eq!(transition_from_message(WM_QUIT), None);
    }
}
