//! AudioService：start/stop/cancel，输出 Recording（06 §4 audio/）。
pub mod chime;
pub mod pipeline;
pub mod recorder;
pub mod vad;

use crate::error::{ErrorCode, Result, TypexError};
use crate::settings::schema::VadSettings;
use crate::types::AudioInputDevice;
use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;
use tokio::sync::{broadcast, mpsc};

/// 一次录音的产物：16 kHz mono WAV + 时长。
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub wav_16k_mono: Vec<u8>,
    pub duration_ms: u64,
    pub vad: VadSettings,
}

/// 电平事件（50ms 节流；HUD 波形数据源）。
pub type LevelSender = mpsc::UnboundedSender<Vec<f32>>;

/// 录音设备在 stream 运行期间失效；供 orchestrator 主动结束 Recording。
#[derive(Debug, Clone)]
pub struct AudioFailure {
    pub session_id: u64,
    pub error: TypexError,
}

/// A promoted candidate that finished opening can persist a migrated device ID.
#[derive(Debug, Clone)]
pub struct AudioReady {
    pub session_id: u64,
    pub migrated_device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidatePromotion {
    Opening,
    Ready(Option<String>),
    NotFound,
}

#[derive(Clone)]
pub(super) struct CaptureGate {
    inner: Arc<Mutex<CaptureGateState>>,
    failure_tx: broadcast::Sender<AudioFailure>,
    started_at: Instant,
}

struct CaptureGateState {
    session_id: Option<u64>,
    levels: Option<LevelSender>,
    pending_failure: Option<TypexError>,
    cancelled: bool,
}

impl CaptureGate {
    fn candidate(failure_tx: broadcast::Sender<AudioFailure>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CaptureGateState {
                session_id: None,
                levels: None,
                pending_failure: None,
                cancelled: false,
            })),
            failure_tx,
            started_at: Instant::now(),
        }
    }

    fn active(
        session_id: u64,
        levels: LevelSender,
        failure_tx: broadcast::Sender<AudioFailure>,
    ) -> Self {
        let gate = Self::candidate(failure_tx);
        gate.promote(session_id, levels);
        gate
    }

    fn promote(&self, session_id: u64, levels: LevelSender) -> Option<TypexError> {
        let mut state = self.inner.lock().unwrap();
        if state.cancelled {
            return Some(TypexError::new(ErrorCode::AudioDevice, "候选录音已取消"));
        }
        state.session_id = Some(session_id);
        state.levels = Some(levels);
        state.pending_failure.take()
    }

    fn cancel(&self) {
        let mut state = self.inner.lock().unwrap();
        state.cancelled = true;
        state.session_id = None;
        state.levels = None;
        state.pending_failure = None;
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.inner.lock().unwrap().cancelled
    }

    pub(super) fn emit_levels(&self, levels: Vec<f32>) {
        let sender = self.inner.lock().unwrap().levels.clone();
        if let Some(sender) = sender {
            let _ = sender.send(levels);
        }
    }

    pub(super) fn report_failure(&self, error: TypexError) {
        let session_id = {
            let mut state = self.inner.lock().unwrap();
            if state.cancelled {
                return;
            }
            match state.session_id {
                Some(session_id) => Some(session_id),
                None => {
                    state.pending_failure = Some(error.clone());
                    None
                }
            }
        };
        if let Some(session_id) = session_id {
            let _ = self.failure_tx.send(AudioFailure { session_id, error });
        }
    }

    pub(super) fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

type CaptureOpener = dyn Fn(&str, CaptureGate, VadSettings) -> Result<(recorder::ActiveRecording, Option<String>)>
    + Send
    + Sync;

enum CaptureState {
    Idle,
    Opening {
        token: u64,
        gate: CaptureGate,
        promoted_session: Option<u64>,
    },
    Candidate {
        token: u64,
        gate: CaptureGate,
        recording: recorder::ActiveRecording,
        migrated_device_id: Option<String>,
    },
    CandidateFailed {
        token: u64,
        error: TypexError,
    },
    Active {
        recording: recorder::ActiveRecording,
    },
    ActiveFailed {
        error: TypexError,
    },
}

struct CaptureShared {
    state: Mutex<CaptureState>,
    changed: Condvar,
}

pub struct AudioService {
    capture: Arc<CaptureShared>,
    failure_tx: broadcast::Sender<AudioFailure>,
    ready_tx: broadcast::Sender<AudioReady>,
    opener: Arc<CaptureOpener>,
}

impl AudioService {
    pub fn new() -> Self {
        let (failure_tx, _) = broadcast::channel(8);
        let (ready_tx, _) = broadcast::channel(8);
        Self {
            capture: Arc::new(CaptureShared {
                state: Mutex::new(CaptureState::Idle),
                changed: Condvar::new(),
            }),
            failure_tx,
            ready_tx,
            opener: Arc::new(recorder::ActiveRecording::start),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_delayed_recording(recording: Recording, delay: std::time::Duration) -> Self {
        let (failure_tx, _) = broadcast::channel(8);
        let (ready_tx, _) = broadcast::channel(8);
        Self {
            capture: Arc::new(CaptureShared {
                state: Mutex::new(CaptureState::Idle),
                changed: Condvar::new(),
            }),
            failure_tx,
            ready_tx,
            opener: Arc::new(move |_, _, _| {
                Ok((
                    recorder::ActiveRecording::fake_delayed(Ok(recording.clone()), delay),
                    None,
                ))
            }),
        }
    }

    /// 开始录音。`device_selection` 空 = 系统默认；返回旧名称迁移后的稳定 ID。
    pub fn start(
        &self,
        session_id: u64,
        device_selection: &str,
        levels: LevelSender,
        vad: VadSettings,
    ) -> Result<Option<String>> {
        let mut state = self.capture.state.lock().unwrap();
        if !matches!(&*state, CaptureState::Idle) {
            return Err(TypexError::new(ErrorCode::Internal, "已有录音进行中"));
        }
        let gate = CaptureGate::active(session_id, levels, self.failure_tx.clone());
        let (recording, migrated_device_id) = (self.opener)(device_selection, gate, vad)?;
        *state = CaptureState::Active { recording };
        Ok(migrated_device_id)
    }

    /// Start an invisible, memory-only candidate without blocking the caller.
    pub fn prepare_candidate(&self, token: u64, device_selection: &str, vad: VadSettings) -> bool {
        let gate = CaptureGate::candidate(self.failure_tx.clone());
        {
            let mut state = self.capture.state.lock().unwrap();
            if !matches!(&*state, CaptureState::Idle) {
                return false;
            }
            *state = CaptureState::Opening {
                token,
                gate: gate.clone(),
                promoted_session: None,
            };
        }

        tracing::debug!(raw_to_candidate_ms = 0_u64);
        let capture = self.capture.clone();
        let ready_tx = self.ready_tx.clone();
        let failure_tx = self.failure_tx.clone();
        let opener = self.opener.clone();
        let device_selection = device_selection.to_string();
        let spawn_result = std::thread::Builder::new()
            .name("typex-audio-prepare".into())
            .spawn(move || {
                let result = opener(&device_selection, gate.clone(), vad);
                let mut ready = None;
                let mut failure = None;
                let mut discard = None;
                {
                    let mut state = capture.state.lock().unwrap();
                    let previous = std::mem::replace(&mut *state, CaptureState::Idle);
                    match previous {
                        CaptureState::Opening {
                            token: current,
                            promoted_session,
                            ..
                        } if current == token => match result {
                            Ok((recording, migrated_device_id)) => {
                                if let Some(session_id) = promoted_session {
                                    ready = Some(AudioReady {
                                        session_id,
                                        migrated_device_id: migrated_device_id.clone(),
                                    });
                                    *state = CaptureState::Active { recording };
                                } else {
                                    *state = CaptureState::Candidate {
                                        token,
                                        gate,
                                        recording,
                                        migrated_device_id,
                                    };
                                }
                            }
                            Err(error) => {
                                if let Some(session_id) = promoted_session {
                                    failure = Some(AudioFailure {
                                        session_id,
                                        error: error.clone(),
                                    });
                                    *state = CaptureState::ActiveFailed { error };
                                } else {
                                    *state = CaptureState::CandidateFailed { token, error };
                                }
                            }
                        },
                        other => {
                            if let Ok((recording, _)) = result {
                                discard = Some(recording);
                            }
                            *state = other;
                        }
                    }
                    capture.changed.notify_all();
                }
                drop(discard);
                if let Some(ready) = ready {
                    let _ = ready_tx.send(ready);
                }
                if let Some(failure) = failure {
                    let _ = failure_tx.send(failure);
                }
            });
        if spawn_result.is_err() {
            let mut state = self.capture.state.lock().unwrap();
            if matches!(
                &*state,
                CaptureState::Opening { token: current, .. } if *current == token
            ) {
                *state = CaptureState::CandidateFailed {
                    token,
                    error: TypexError::new(ErrorCode::Internal, "候选音频线程启动失败"),
                };
                self.capture.changed.notify_all();
            }
        }
        true
    }

    pub fn promote_candidate(
        &self,
        token: u64,
        session_id: u64,
        levels: LevelSender,
    ) -> Result<CandidatePromotion> {
        let mut state = self.capture.state.lock().unwrap();
        let previous = std::mem::replace(&mut *state, CaptureState::Idle);
        match previous {
            CaptureState::Opening {
                token: current,
                gate,
                promoted_session,
            } if current == token && promoted_session.is_none() => {
                if let Some(error) = gate.promote(session_id, levels) {
                    gate.cancel();
                    self.capture.changed.notify_all();
                    return Err(error);
                }
                *state = CaptureState::Opening {
                    token,
                    gate,
                    promoted_session: Some(session_id),
                };
                Ok(CandidatePromotion::Opening)
            }
            CaptureState::Candidate {
                token: current,
                gate,
                recording,
                migrated_device_id,
            } if current == token => {
                if let Some(error) = gate.promote(session_id, levels) {
                    gate.cancel();
                    drop(recording);
                    self.capture.changed.notify_all();
                    return Err(error);
                }
                let migrated = migrated_device_id.clone();
                *state = CaptureState::Active { recording };
                Ok(CandidatePromotion::Ready(migrated))
            }
            CaptureState::CandidateFailed {
                token: current,
                error,
            } if current == token => Err(error),
            other => {
                *state = other;
                Ok(CandidatePromotion::NotFound)
            }
        }
    }

    pub fn cancel_candidate(&self, token: u64) {
        self.cancel_candidate_matching(Some(token));
    }

    pub fn cancel_pending_candidate(&self) {
        self.cancel_candidate_matching(None);
    }

    fn cancel_candidate_matching(&self, token: Option<u64>) {
        let mut state = self.capture.state.lock().unwrap();
        let previous = std::mem::replace(&mut *state, CaptureState::Idle);
        let mut recording = None;
        match previous {
            CaptureState::Opening {
                token: current,
                gate,
                promoted_session: None,
            } if token.is_none_or(|token| token == current) => gate.cancel(),
            CaptureState::Candidate {
                token: current,
                gate,
                recording: candidate,
                ..
            } if token.is_none_or(|token| token == current) => {
                gate.cancel();
                recording = Some(candidate);
            }
            CaptureState::CandidateFailed { token: current, .. }
                if token.is_none_or(|token| token == current) => {}
            other => *state = other,
        }
        self.capture.changed.notify_all();
        drop(state);
        drop(recording);
    }

    /// 停止并取回 WAV。
    pub fn stop(&self) -> Result<Recording> {
        let mut state = self.capture.state.lock().unwrap();
        loop {
            let previous = std::mem::replace(&mut *state, CaptureState::Idle);
            match previous {
                CaptureState::Active { recording } => {
                    drop(state);
                    return recording.finish();
                }
                CaptureState::ActiveFailed { error } => return Err(error),
                opening @ CaptureState::Opening {
                    promoted_session: Some(_),
                    ..
                } => {
                    *state = opening;
                    state = self.capture.changed.wait(state).unwrap();
                }
                other => {
                    *state = other;
                    return Err(TypexError::new(ErrorCode::Internal, "没有进行中的录音"));
                }
            }
        }
    }

    /// 放弃本次录音，不产生输出。
    pub fn cancel(&self) {
        let mut state = self.capture.state.lock().unwrap();
        let previous = std::mem::replace(&mut *state, CaptureState::Idle);
        let recording = match previous {
            CaptureState::Opening { gate, .. } => {
                gate.cancel();
                None
            }
            CaptureState::Candidate {
                gate, recording, ..
            } => {
                gate.cancel();
                Some(recording)
            }
            CaptureState::Active { recording } => Some(recording),
            CaptureState::Idle
            | CaptureState::CandidateFailed { .. }
            | CaptureState::ActiveFailed { .. } => None,
        };
        self.capture.changed.notify_all();
        drop(state);
        drop(recording);
    }

    pub fn is_recording(&self) -> bool {
        matches!(
            &*self.capture.state.lock().unwrap(),
            CaptureState::Active { .. }
                | CaptureState::ActiveFailed { .. }
                | CaptureState::Opening {
                    promoted_session: Some(_),
                    ..
                }
        )
    }

    pub fn subscribe_failures(&self) -> broadcast::Receiver<AudioFailure> {
        self.failure_tx.subscribe()
    }

    pub fn subscribe_ready(&self) -> broadcast::Receiver<AudioReady> {
        self.ready_tx.subscribe()
    }
}

impl Default for AudioService {
    fn default() -> Self {
        Self::new()
    }
}

/// 枚举输入设备稳定 ID 与展示名称（设置页麦克风下拉）。
pub fn list_input_devices() -> Result<Vec<AudioInputDevice>> {
    let host = cpal::default_host();
    enumerate_input_devices(&host)
        .map(|devices| devices.into_iter().map(|(_, info)| info).collect())
}

pub(super) fn enumerate_input_devices(
    host: &cpal::Host,
) -> Result<Vec<(cpal::Device, AudioInputDevice)>> {
    let devices = host
        .input_devices()
        .map_err(|_| TypexError::new(ErrorCode::AudioDevice, "无法枚举输入设备"))?;
    let mut result = Vec::new();
    for device in devices {
        let (Ok(id), Ok(label)) = (device.id(), device.name()) else {
            tracing::warn!("跳过无法读取标识或名称的输入设备");
            continue;
        };
        result.push((device, AudioInputDevice { id, label }));
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DeviceResolution {
    Default,
    Fixed {
        index: usize,
        migrated_device_id: Option<String>,
    },
}

pub(super) fn resolve_device_selection(
    selection: &str,
    devices: &[AudioInputDevice],
) -> Result<DeviceResolution> {
    if selection.is_empty() {
        return Ok(DeviceResolution::Default);
    }

    if let Some(index) = devices.iter().position(|device| device.id == selection) {
        return Ok(DeviceResolution::Fixed {
            index,
            migrated_device_id: None,
        });
    }

    let mut legacy_matches = devices
        .iter()
        .enumerate()
        .filter(|(_, device)| device.label == selection);
    let Some((index, device)) = legacy_matches.next() else {
        return Err(TypexError::new(
            ErrorCode::AudioDevice,
            "固定的输入设备不可用，请重新选择麦克风",
        ));
    };
    if legacy_matches.next().is_some() {
        return Err(TypexError::new(
            ErrorCode::AudioDevice,
            "旧麦克风名称匹配多个设备，请重新选择麦克风",
        ));
    }

    Ok(DeviceResolution::Fixed {
        index,
        migrated_device_id: Some(device.id.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_recording() -> Recording {
        Recording {
            wav_16k_mono: Vec::new(),
            duration_ms: 100,
            vad: VadSettings::default(),
        }
    }

    fn service_with_opener(
        opener: impl Fn(
            &str,
            CaptureGate,
            VadSettings,
        ) -> Result<(recorder::ActiveRecording, Option<String>)>
        + Send
        + Sync
        + 'static,
    ) -> AudioService {
        let (failure_tx, _) = broadcast::channel(8);
        let (ready_tx, _) = broadcast::channel(8);
        AudioService {
            capture: Arc::new(CaptureShared {
                state: Mutex::new(CaptureState::Idle),
                changed: Condvar::new(),
            }),
            failure_tx,
            ready_tx,
            opener: Arc::new(opener),
        }
    }

    fn wait_for_candidate(service: &AudioService) {
        let mut state = service.capture.state.lock().unwrap();
        for _ in 0..100 {
            if !matches!(&*state, CaptureState::Opening { .. }) {
                return;
            }
            let (next, _) = service
                .capture
                .changed
                .wait_timeout(state, std::time::Duration::from_millis(20))
                .unwrap();
            state = next;
        }
        panic!("candidate did not settle");
    }

    fn devices() -> Vec<AudioInputDevice> {
        vec![
            AudioInputDevice {
                id: "endpoint-a".into(),
                label: "USB Microphone".into(),
            },
            AudioInputDevice {
                id: "endpoint-b".into(),
                label: "Built-in Microphone".into(),
            },
        ]
    }

    #[test]
    fn empty_device_selection_follows_system_default() {
        assert_eq!(
            resolve_device_selection("", &devices()).unwrap(),
            DeviceResolution::Default
        );
    }

    #[test]
    fn stable_endpoint_id_selects_exact_device_without_migration() {
        assert_eq!(
            resolve_device_selection("endpoint-b", &devices()).unwrap(),
            DeviceResolution::Fixed {
                index: 1,
                migrated_device_id: None,
            }
        );
    }

    #[test]
    fn unique_legacy_display_name_migrates_to_endpoint_id() {
        assert_eq!(
            resolve_device_selection("USB Microphone", &devices()).unwrap(),
            DeviceResolution::Fixed {
                index: 0,
                migrated_device_id: Some("endpoint-a".into()),
            }
        );
    }

    #[test]
    fn missing_fixed_device_is_not_replaced_by_default() {
        let error = resolve_device_selection("removed-endpoint", &devices()).unwrap_err();
        assert_eq!(error.code, ErrorCode::AudioDevice);
    }

    #[test]
    fn ambiguous_legacy_display_name_fails_explicitly() {
        let devices = vec![
            AudioInputDevice {
                id: "endpoint-a".into(),
                label: "Microphone".into(),
            },
            AudioInputDevice {
                id: "endpoint-b".into(),
                label: "Microphone".into(),
            },
        ];
        let error = resolve_device_selection("Microphone", &devices).unwrap_err();
        assert_eq!(error.code, ErrorCode::AudioDevice);
        assert!(error.message.contains("多个设备"));
    }

    #[test]
    fn candidate_tokens_isolate_prepare_promote_and_cancel() {
        let service = service_with_opener(|_, _, _| {
            Ok((
                recorder::ActiveRecording::fake(Ok(fake_recording())),
                Some("endpoint-migrated".into()),
            ))
        });
        let (levels, _level_rx) = mpsc::unbounded_channel();

        assert!(service.prepare_candidate(11, "", VadSettings::default()));
        wait_for_candidate(&service);
        assert_eq!(
            service.promote_candidate(12, 7, levels.clone()).unwrap(),
            CandidatePromotion::NotFound
        );
        service.cancel_candidate(12);
        assert_eq!(
            service.promote_candidate(11, 7, levels).unwrap(),
            CandidatePromotion::Ready(Some("endpoint-migrated".into()))
        );
        assert!(service.is_recording());
        service.cancel();
    }

    #[test]
    fn candidate_levels_are_gated_until_promotion() {
        let captured_gate = Arc::new(Mutex::new(None::<CaptureGate>));
        let gate_out = captured_gate.clone();
        let service = service_with_opener(move |_, gate, _| {
            *gate_out.lock().unwrap() = Some(gate);
            Ok((recorder::ActiveRecording::fake(Ok(fake_recording())), None))
        });
        let (levels, mut level_rx) = mpsc::unbounded_channel();

        assert!(service.prepare_candidate(21, "", VadSettings::default()));
        wait_for_candidate(&service);
        let gate = captured_gate.lock().unwrap().clone().unwrap();
        gate.emit_levels(vec![0.1]);
        assert!(level_rx.try_recv().is_err());

        assert_eq!(
            service.promote_candidate(21, 8, levels).unwrap(),
            CandidatePromotion::Ready(None)
        );
        gate.emit_levels(vec![0.2]);
        assert_eq!(level_rx.try_recv().unwrap(), vec![0.2]);
        service.cancel();
    }

    #[test]
    fn cancellation_during_open_discards_late_stream() {
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let (finished_tx, finished_rx) = std::sync::mpsc::channel::<()>();
        let service = service_with_opener(move |_, _, _| {
            let _ = release_rx.lock().unwrap().recv();
            let _ = finished_tx.send(());
            Ok((recorder::ActiveRecording::fake(Ok(fake_recording())), None))
        });

        assert!(service.prepare_candidate(31, "", VadSettings::default()));
        service.cancel_candidate(31);
        release_tx.send(()).unwrap();
        finished_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!service.is_recording());
        let (levels, _level_rx) = mpsc::unbounded_channel();
        assert_eq!(
            service.promote_candidate(31, 9, levels).unwrap(),
            CandidatePromotion::NotFound
        );
    }

    #[test]
    fn promotion_before_stream_ready_reuses_opening_capture() {
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let service = service_with_opener(move |_, _, _| {
            let _ = release_rx.lock().unwrap().recv();
            Ok((
                recorder::ActiveRecording::fake(Ok(fake_recording())),
                Some("endpoint-ready".into()),
            ))
        });
        let mut ready_rx = service.subscribe_ready();
        let (levels, _level_rx) = mpsc::unbounded_channel();

        assert!(service.prepare_candidate(41, "", VadSettings::default()));
        assert_eq!(
            service.promote_candidate(41, 10, levels).unwrap(),
            CandidatePromotion::Opening
        );
        assert!(service.is_recording());
        release_tx.send(()).unwrap();
        let ready = ready_rx.blocking_recv().unwrap();
        assert_eq!(ready.session_id, 10);
        assert_eq!(ready.migrated_device_id.as_deref(), Some("endpoint-ready"));
        assert_eq!(service.stop().unwrap().duration_ms, 100);
    }

    #[test]
    fn candidate_start_failure_is_silent_until_confirmed() {
        let service = service_with_opener(|_, _, _| {
            Err(TypexError::new(ErrorCode::AudioDevice, "open failed"))
        });
        let mut failures = service.subscribe_failures();
        let (levels, _level_rx) = mpsc::unbounded_channel();

        assert!(service.prepare_candidate(51, "", VadSettings::default()));
        wait_for_candidate(&service);
        assert!(failures.try_recv().is_err());
        let error = service.promote_candidate(51, 11, levels).unwrap_err();
        assert_eq!(error.code, ErrorCode::AudioDevice);
    }

    #[test]
    fn confirmed_opening_failure_keeps_the_original_error_for_fast_stop() {
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let service = service_with_opener(move |_, _, _| {
            let _ = release_rx.lock().unwrap().recv();
            Err(TypexError::new(ErrorCode::AudioDevice, "open failed"))
        });
        let mut failures = service.subscribe_failures();
        let (levels, _level_rx) = mpsc::unbounded_channel();

        assert!(service.prepare_candidate(56, "", VadSettings::default()));
        assert_eq!(
            service.promote_candidate(56, 15, levels).unwrap(),
            CandidatePromotion::Opening
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            failures.blocking_recv().unwrap().error.code,
            ErrorCode::AudioDevice
        );
        assert!(service.is_recording());
        assert_eq!(service.stop().unwrap_err().code, ErrorCode::AudioDevice);
    }

    #[test]
    fn candidate_runtime_failure_is_deferred_but_active_failure_is_reported() {
        let captured_gate = Arc::new(Mutex::new(None::<CaptureGate>));
        let gate_out = captured_gate.clone();
        let service = service_with_opener(move |_, gate, _| {
            *gate_out.lock().unwrap() = Some(gate);
            Ok((recorder::ActiveRecording::fake(Ok(fake_recording())), None))
        });
        let mut failures = service.subscribe_failures();
        let (levels, _level_rx) = mpsc::unbounded_channel();

        assert!(service.prepare_candidate(61, "", VadSettings::default()));
        wait_for_candidate(&service);
        let gate = captured_gate.lock().unwrap().clone().unwrap();
        gate.report_failure(TypexError::new(ErrorCode::AudioDevice, "runtime"));
        assert!(failures.try_recv().is_err());
        let error = service
            .promote_candidate(61, 12, levels.clone())
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AudioDevice);

        service.cancel();
        let active_gate = CaptureGate::active(13, levels, service.failure_tx.clone());
        active_gate.report_failure(TypexError::new(ErrorCode::AudioDevice, "runtime"));
        assert_eq!(failures.blocking_recv().unwrap().session_id, 13);
    }
}
