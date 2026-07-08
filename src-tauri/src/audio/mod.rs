//! AudioService：start/stop/cancel，输出 Recording（06 §4 audio/）。
pub mod chime;
pub mod pipeline;
pub mod recorder;
pub mod vad;

use crate::error::{ErrorCode, Result, TypexError};
use std::sync::Mutex;
use tokio::sync::mpsc;

/// 一次录音的产物：16 kHz mono WAV + 时长。
#[derive(Debug, Clone)]
pub struct Recording {
    pub wav_16k_mono: Vec<u8>,
    pub duration_ms: u64,
}

/// 电平事件（50ms 节流；HUD 波形数据源）。
pub type LevelSender = mpsc::UnboundedSender<Vec<f32>>;

pub struct AudioService {
    active: Mutex<Option<recorder::ActiveRecording>>,
}

impl AudioService {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    /// 开始录音。`device_name` 空 = 系统默认；电平经 `levels` 推送。
    pub fn start(&self, device_name: &str, levels: LevelSender) -> Result<()> {
        let mut guard = self.active.lock().unwrap();
        if guard.is_some() {
            return Err(TypexError::new(ErrorCode::Internal, "已有录音进行中"));
        }
        *guard = Some(recorder::ActiveRecording::start(device_name, levels)?);
        Ok(())
    }

    /// 停止并取回 WAV。
    pub fn stop(&self) -> Result<Recording> {
        let mut guard = self.active.lock().unwrap();
        match guard.take() {
            Some(rec) => rec.finish(),
            None => Err(TypexError::new(ErrorCode::Internal, "没有进行中的录音")),
        }
    }

    /// 放弃本次录音，不产生输出。
    pub fn cancel(&self) {
        let mut guard = self.active.lock().unwrap();
        *guard = None; // drop 即停流
    }

    pub fn is_recording(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }
}

impl Default for AudioService {
    fn default() -> Self {
        Self::new()
    }
}

/// 枚举输入设备名列表（设置页麦克风下拉）。
pub fn list_input_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}
