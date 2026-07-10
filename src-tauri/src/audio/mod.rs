//! AudioService：start/stop/cancel，输出 Recording（06 §4 audio/）。
pub mod chime;
pub mod pipeline;
pub mod recorder;
pub mod vad;

use crate::error::{ErrorCode, Result, TypexError};
use crate::types::AudioInputDevice;
use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::Mutex;
use tokio::sync::{broadcast, mpsc};

/// 一次录音的产物：16 kHz mono WAV + 时长。
#[derive(Debug, Clone)]
pub struct Recording {
    pub wav_16k_mono: Vec<u8>,
    pub duration_ms: u64,
}

/// 电平事件（50ms 节流；HUD 波形数据源）。
pub type LevelSender = mpsc::UnboundedSender<Vec<f32>>;

/// 录音设备在 stream 运行期间失效；供 orchestrator 主动结束 Recording。
#[derive(Debug, Clone)]
pub struct AudioFailure {
    pub session_id: u64,
    pub error: TypexError,
}

pub struct AudioService {
    active: Mutex<Option<recorder::ActiveRecording>>,
    failure_tx: broadcast::Sender<AudioFailure>,
}

impl AudioService {
    pub fn new() -> Self {
        let (failure_tx, _) = broadcast::channel(8);
        Self {
            active: Mutex::new(None),
            failure_tx,
        }
    }

    /// 开始录音。`device_selection` 空 = 系统默认；返回旧名称迁移后的稳定 ID。
    pub fn start(
        &self,
        session_id: u64,
        device_selection: &str,
        levels: LevelSender,
    ) -> Result<Option<String>> {
        let mut guard = self.active.lock().unwrap();
        if guard.is_some() {
            return Err(TypexError::new(ErrorCode::Internal, "已有录音进行中"));
        }
        let (recording, migrated_device_id) = recorder::ActiveRecording::start(
            session_id,
            device_selection,
            levels,
            self.failure_tx.clone(),
        )?;
        *guard = Some(recording);
        Ok(migrated_device_id)
    }

    /// 停止并取回 WAV。
    pub fn stop(&self) -> Result<Recording> {
        let recording = self.active.lock().unwrap().take();
        match recording {
            Some(rec) => rec.finish(),
            None => Err(TypexError::new(ErrorCode::Internal, "没有进行中的录音")),
        }
    }

    /// 放弃本次录音，不产生输出。
    pub fn cancel(&self) {
        let recording = self.active.lock().unwrap().take();
        drop(recording); // drop 即停流；等待 worker 时不占用服务锁
    }

    pub fn is_recording(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }

    pub fn subscribe_failures(&self) -> broadcast::Receiver<AudioFailure> {
        self.failure_tx.subscribe()
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
}
