//! cpal 流 + 采样收集（07 §7.4）。
//!
//! callback 内只做「拷贝进 channel」（实时线程禁止分配大块/锁/日志）；
//! 重采样与电平计算在 worker 线程（pipeline.rs）。

use super::{LevelSender, Recording, pipeline};
use crate::error::{ErrorCode, Result, TypexError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc as std_mpsc;
use std::time::Instant;

pub struct ActiveRecording {
    // Stream 不是 Send；持有 stop 信号，流本体活在专属线程
    stop_tx: std_mpsc::Sender<()>,
    result_rx: std_mpsc::Receiver<Result<Recording>>,
}

impl ActiveRecording {
    pub fn start(device_name: &str, levels: LevelSender) -> Result<Self> {
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        let (result_tx, result_rx) = std_mpsc::channel::<Result<Recording>>();
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<()>>();
        let device_name = device_name.to_string();

        std::thread::Builder::new()
            .name("typex-audio".into())
            .spawn(move || {
                run_stream(&device_name, levels, stop_rx, ready_tx, result_tx);
            })
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("音频线程启动失败: {e}")))?;

        // 等待流实际打开（错误此刻返回，如无麦克风权限/设备不存在）
        ready_rx
            .recv()
            .map_err(|_| TypexError::new(ErrorCode::AudioDevice, "音频线程异常退出"))??;

        Ok(Self { stop_tx, result_rx })
    }

    pub fn finish(self) -> Result<Recording> {
        let _ = self.stop_tx.send(());
        // duration 以 VAD 裁剪后的有效时长为准（pipeline::finalize_recording）
        self.result_rx
            .recv()
            .map_err(|_| TypexError::new(ErrorCode::AudioDevice, "音频线程异常退出"))?
    }
}

fn run_stream(
    device_name: &str,
    levels: LevelSender,
    stop_rx: std_mpsc::Receiver<()>,
    ready_tx: std_mpsc::Sender<Result<()>>,
    result_tx: std_mpsc::Sender<Result<Recording>>,
) {
    let host = cpal::default_host();
    let device = if device_name.is_empty() {
        host.default_input_device()
    } else {
        host.input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().is_ok_and(|n| n == device_name)))
            .or_else(|| host.default_input_device())
    };
    let Some(device) = device else {
        let _ = ready_tx.send(Err(TypexError::new(
            ErrorCode::AudioDevice,
            "找不到输入设备",
        )));
        return;
    };
    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready_tx.send(Err(TypexError::new(
                ErrorCode::AudioDevice,
                format!("读取设备配置失败: {e}"),
            )));
            return;
        }
    };
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    // callback → worker 的原始采样通道
    let (raw_tx, raw_rx) = std_mpsc::channel::<Vec<f32>>();

    let stream = match device.build_input_stream(
        &config.into(),
        move |data: &[f32], _| {
            // 实时回调：仅拷贝转发
            let _ = raw_tx.send(data.to_vec());
        },
        |e| tracing::warn!("音频流错误: {e}"),
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            let _ = ready_tx.send(Err(TypexError::new(
                ErrorCode::AudioDevice,
                format!("打开音频流失败: {e}"),
            )));
            return;
        }
    };
    if let Err(e) = stream.play() {
        let _ = ready_tx.send(Err(TypexError::new(
            ErrorCode::AudioDevice,
            format!("启动音频流失败: {e}"),
        )));
        return;
    }
    let _ = ready_tx.send(Ok(()));

    // worker：本线程消费原始采样 → 混单声道 → 攒样本；电平 50ms 节流
    let mut mono: Vec<f32> = Vec::with_capacity(sample_rate as usize * 30);
    let mut level_window: Vec<f32> = Vec::new();
    let mut last_level = Instant::now();
    loop {
        // stop 信号优先
        if stop_rx.try_recv().is_ok() {
            break;
        }
        match raw_rx.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(chunk) => {
                for frame in chunk.chunks(channels) {
                    let s = frame.iter().sum::<f32>() / channels as f32;
                    mono.push(s);
                    level_window.push(s);
                }
                if last_level.elapsed().as_millis() >= 50 {
                    let levels_vec = pipeline::rms_levels(&level_window, 12);
                    let _ = levels.send(levels_vec);
                    level_window.clear();
                    last_level = Instant::now();
                }
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stream);

    let result = pipeline::finalize_recording(&mono, sample_rate);
    let _ = result_tx.send(result.map(|(wav, duration_ms)| Recording {
        wav_16k_mono: wav,
        duration_ms,
    }));
}
