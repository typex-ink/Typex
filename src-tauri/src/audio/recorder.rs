//! cpal 流 + 采样收集（06 §7.4）。
//!
//! callback 只把设备原始采样复制到预分配的有界 SPSC ring buffer；
//! 格式转换、单声道混合、电平计算与重采样均留在 worker 线程。

use super::{
    CaptureGate, DeviceResolution, Recording, enumerate_input_devices, pipeline,
    resolve_device_selection,
};
use crate::error::{ErrorCode, Result, TypexError};
use crate::settings::schema::VadSettings;
use crate::types::AudioInputDevice;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc as std_mpsc};
use std::time::{Duration, Instant};

const BUFFER_MILLIS: usize = 1_000;
const MIN_BUFFER_FRAMES: usize = 1_024;
const MAX_BUFFER_SAMPLES: usize = 2_000_000;
const WORKER_BATCH_FRAMES: usize = 4_096;

pub(super) struct ActiveRecording {
    // Stream 不是 Send；持有 stop 信号，流本体活在专属线程。
    stop_tx: std_mpsc::Sender<StopSignal>,
    result_rx: std_mpsc::Receiver<Result<Recording>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone, Copy)]
enum StopSignal {
    Finish,
    Cancel,
}

struct StreamControl {
    migrated_device_id: Option<String>,
    gate: CaptureGate,
    stop_rx: std_mpsc::Receiver<StopSignal>,
    ready_tx: std_mpsc::Sender<Result<Option<String>>>,
    result_tx: std_mpsc::Sender<Result<Recording>>,
    vad: VadSettings,
}

impl ActiveRecording {
    pub(super) fn start(
        device_selection: &str,
        gate: CaptureGate,
        vad: VadSettings,
    ) -> Result<(Self, Option<String>)> {
        let (stop_tx, stop_rx) = std_mpsc::channel::<StopSignal>();
        let (result_tx, result_rx) = std_mpsc::channel::<Result<Recording>>();
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<Option<String>>>();
        let device_selection = device_selection.to_string();

        let worker = std::thread::Builder::new()
            .name("typex-audio".into())
            .spawn(move || {
                run_stream(&device_selection, gate, stop_rx, ready_tx, result_tx, vad);
            })
            .map_err(|_| TypexError::new(ErrorCode::Internal, "音频线程启动失败"))?;

        // 等待流实际打开（错误此刻返回，如无麦克风权限/设备不存在）。
        let migrated_device_id = ready_rx
            .recv()
            .map_err(|_| TypexError::new(ErrorCode::AudioDevice, "音频线程异常退出"))??;

        Ok((
            Self {
                stop_tx,
                result_rx,
                worker: Some(worker),
            },
            migrated_device_id,
        ))
    }

    pub fn finish(mut self) -> Result<Recording> {
        let _ = self.stop_tx.send(StopSignal::Finish);
        // duration 以 VAD 裁剪后的有效时长为准（pipeline::finalize_recording）。
        let result = self
            .result_rx
            .recv()
            .unwrap_or_else(|_| Err(TypexError::new(ErrorCode::AudioDevice, "音频线程异常退出")));
        self.join_worker();
        result
    }

    fn join_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    #[cfg(test)]
    pub(super) fn fake(result: Result<Recording>) -> Self {
        let (stop_tx, _stop_rx) = std_mpsc::channel();
        let (result_tx, result_rx) = std_mpsc::channel();
        let _ = result_tx.send(result);
        Self {
            stop_tx,
            result_rx,
            worker: None,
        }
    }
}

impl Drop for ActiveRecording {
    fn drop(&mut self) {
        // cancel 路径只需唤醒 worker；线程会自行 drop cpal Stream。
        let _ = self.stop_tx.send(StopSignal::Cancel);
        self.join_worker();
    }
}

fn run_stream(
    device_selection: &str,
    gate: CaptureGate,
    stop_rx: std_mpsc::Receiver<StopSignal>,
    ready_tx: std_mpsc::Sender<Result<Option<String>>>,
    result_tx: std_mpsc::Sender<Result<Recording>>,
    vad: VadSettings,
) {
    if gate.is_cancelled() {
        send_ready_error(&ready_tx, "候选录音已取消");
        return;
    }
    let host = cpal::default_host();
    let (device, migrated_device_id) = match select_input_device(&host, device_selection) {
        Ok(selected) => selected,
        Err(error) => {
            let _ = ready_tx.send(Err(error));
            return;
        }
    };
    let supported_config = match device.default_input_config() {
        Ok(config) => config,
        Err(_) => {
            send_ready_error(&ready_tx, "读取输入设备配置失败");
            return;
        }
    };
    let sample_format = supported_config.sample_format();
    let config = supported_config.config();
    let control = StreamControl {
        migrated_device_id,
        gate,
        stop_rx,
        ready_tx,
        result_tx,
        vad,
    };

    match sample_format {
        SampleFormat::F32 => run_typed::<f32>(device, config, control),
        SampleFormat::I16 => run_typed::<i16>(device, config, control),
        SampleFormat::U16 => run_typed::<u16>(device, config, control),
        // SampleFormat 是 non_exhaustive；新格式必须显式增加归一化规则后才能接入。
        _ => send_ready_error(
            &control.ready_tx,
            &format!("不支持的输入采样格式: {sample_format}"),
        ),
    }
}

fn run_typed<T>(device: cpal::Device, config: cpal::StreamConfig, control: StreamControl)
where
    T: InputSample,
{
    let StreamControl {
        migrated_device_id,
        gate,
        stop_rx,
        ready_tx,
        result_tx,
        vad,
    } = control;
    let sample_rate = config.sample_rate.0;
    let channels = usize::from(config.channels);
    if channels == 0 || sample_rate == 0 {
        send_ready_error(&ready_tx, "输入设备配置无效");
        return;
    }

    let ring = Arc::new(SampleRing::<T>::new(buffer_capacity(sample_rate, channels)));
    let callback_ring = Arc::clone(&ring);
    let (stream_error_tx, stream_error_rx) = std_mpsc::sync_channel(1);

    let stream = match device.build_input_stream(
        &config,
        move |data: &[T], _| {
            callback_ring.push_frames(data, channels);
        },
        move |error| {
            // 保留首个流错误即可；不能从驱动回调里阻塞或写日志。
            let _ = stream_error_tx.try_send(error);
        },
        None,
    ) {
        Ok(stream) => stream,
        Err(_) => {
            send_ready_error(&ready_tx, "打开输入音频流失败");
            return;
        }
    };
    if stream.play().is_err() {
        send_ready_error(&ready_tx, "启动输入音频流失败");
        return;
    }
    tracing::debug!(stream_ready_ms = gate.elapsed_ms());
    let _ = ready_tx.send(Ok(migrated_device_id));

    // worker：消费原始采样 → 格式归一化 → 混单声道 → 攒样本。
    let mut mono = Vec::with_capacity(sample_rate as usize * 30);
    let mut level_window = Vec::with_capacity(sample_rate as usize / 10);
    let mut raw = Vec::with_capacity(WORKER_BATCH_FRAMES * channels);
    let mut last_level = Instant::now();
    let mut stream_failure = None;
    let mut cancelled = false;
    let mut first_callback_observed = false;

    loop {
        if gate.is_cancelled() {
            cancelled = true;
            break;
        }
        if let Ok(error) = stream_error_rx.try_recv() {
            stream_failure = Some(classify_stream_error(error));
            break;
        }
        match stop_rx.try_recv() {
            Ok(StopSignal::Finish) => break,
            Ok(StopSignal::Cancel) | Err(std_mpsc::TryRecvError::Disconnected) => {
                cancelled = true;
                break;
            }
            Err(std_mpsc::TryRecvError::Empty) => {}
        }

        let frames = ring.drain_frames(channels, WORKER_BATCH_FRAMES, &mut raw);
        if frames == 0 {
            match stop_rx.recv_timeout(Duration::from_millis(5)) {
                Ok(StopSignal::Finish) => break,
                Ok(StopSignal::Cancel) | Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                    cancelled = true;
                    break;
                }
                Err(std_mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }

        if !first_callback_observed {
            first_callback_observed = true;
            tracing::debug!(first_audio_callback_ms = gate.elapsed_ms());
        }

        append_mono(&raw, channels, &mut mono, &mut level_window);
        if last_level.elapsed() >= Duration::from_millis(50) {
            let levels_vec = pipeline::rms_levels(&level_window, 12);
            gate.emit_levels(levels_vec);
            level_window.clear();
            last_level = Instant::now();
        }
    }
    drop(stream);

    let dropped_samples = ring.dropped_samples();
    if dropped_samples != 0 {
        tracing::warn!(dropped_samples, "音频回调缓冲溢出");
    }
    if cancelled {
        return;
    }
    if let Some(error) = stream_failure {
        gate.report_failure(error.clone());
        let _ = result_tx.send(Err(error));
        return;
    }

    // Stream 已关闭，不会再有 producer；排空松键前已经发布的尾部帧。
    while ring.drain_frames(channels, WORKER_BATCH_FRAMES, &mut raw) != 0 {
        append_mono(&raw, channels, &mut mono, &mut level_window);
    }

    let result = pipeline::finalize_recording(&mono, sample_rate, vad);
    let _ = result_tx.send(result.map(|(wav, duration_ms)| Recording {
        wav_16k_mono: wav,
        duration_ms,
        vad,
    }));
}

fn select_input_device(
    host: &cpal::Host,
    selection: &str,
) -> Result<(cpal::Device, Option<String>)> {
    if selection.is_empty() {
        let device = host
            .default_input_device()
            .ok_or_else(|| TypexError::new(ErrorCode::AudioDevice, "找不到系统默认输入设备"))?;
        return Ok((device, None));
    }

    let candidates = enumerate_input_devices(host)?;
    let descriptors: Vec<AudioInputDevice> = candidates
        .iter()
        .map(|(_, descriptor)| descriptor.clone())
        .collect();
    let DeviceResolution::Fixed {
        index,
        migrated_device_id,
    } = resolve_device_selection(selection, &descriptors)?
    else {
        unreachable!("non-empty selection cannot resolve to default")
    };
    let device = candidates
        .into_iter()
        .nth(index)
        .map(|(device, _)| device)
        .ok_or_else(|| TypexError::new(ErrorCode::AudioDevice, "固定的输入设备不可用"))?;
    Ok((device, migrated_device_id))
}

fn send_ready_error(ready_tx: &std_mpsc::Sender<Result<Option<String>>>, message: &str) {
    let _ = ready_tx.send(Err(TypexError::new(ErrorCode::AudioDevice, message)));
}

fn classify_stream_error(error: cpal::StreamError) -> TypexError {
    let message = match error {
        cpal::StreamError::DeviceNotAvailable => "输入设备已不可用，请重新开始录音",
        cpal::StreamError::BackendSpecific { .. } => "输入音频流已中断，请重新开始录音",
    };
    TypexError::new(ErrorCode::AudioDevice, message)
}

fn buffer_capacity(sample_rate: u32, channels: usize) -> usize {
    let max_frames = (MAX_BUFFER_SAMPLES / channels).max(1);
    let desired_frames = (sample_rate as usize).saturating_mul(BUFFER_MILLIS) / 1_000;
    desired_frames
        .clamp(MIN_BUFFER_FRAMES.min(max_frames), max_frames)
        .saturating_mul(channels)
}

trait InputSample: SizedSample + Copy + Default + Send + 'static {
    fn normalized(self) -> f32;
}

impl InputSample for f32 {
    fn normalized(self) -> f32 {
        if self.is_finite() {
            self.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}

impl InputSample for i16 {
    fn normalized(self) -> f32 {
        if self < 0 {
            self as f32 / 32_768.0
        } else {
            self as f32 / 32_767.0
        }
    }
}

impl InputSample for u16 {
    fn normalized(self) -> f32 {
        let centered = i32::from(self) - 32_768;
        if centered < 0 {
            centered as f32 / 32_768.0
        } else {
            centered as f32 / 32_767.0
        }
    }
}

fn append_mono<T: InputSample>(
    interleaved: &[T],
    channels: usize,
    mono: &mut Vec<f32>,
    level_window: &mut Vec<f32>,
) {
    if channels == 0 {
        return;
    }
    for frame in interleaved.chunks_exact(channels) {
        let mixed = frame.iter().map(|sample| sample.normalized()).sum::<f32>() / channels as f32;
        let mixed = mixed.clamp(-1.0, 1.0);
        mono.push(mixed);
        level_window.push(mixed);
    }
}

/// 预分配的单生产者/单消费者环形缓冲。
struct SampleRing<T> {
    slots: Box<[UnsafeCell<T>]>,
    write: AtomicUsize,
    read: AtomicUsize,
    dropped: AtomicU64,
}

// Safety: 只有 cpal data callback 写入，只有 audio worker 读取；write/read 的
// Release/Acquire 发布每一批槽位，读写游标确保同一槽位不会并发访问。
unsafe impl<T: Copy + Send> Sync for SampleRing<T> {}

impl<T: Copy + Default> SampleRing<T> {
    fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        let mut slots = Vec::with_capacity(capacity);
        slots.resize_with(capacity, || UnsafeCell::new(T::default()));
        Self {
            slots: slots.into_boxed_slice(),
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    fn push_frames(&self, input: &[T], channels: usize) {
        if channels == 0 {
            self.add_dropped(input.len());
            return;
        }
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let used = write.wrapping_sub(read).min(self.slots.len());
        let free_frames = (self.slots.len() - used) / channels;
        let input_frames = input.len() / channels;
        let copied_samples = input_frames.min(free_frames) * channels;

        for (offset, sample) in input.iter().copied().take(copied_samples).enumerate() {
            let slot = (write.wrapping_add(offset)) % self.slots.len();
            // Safety: the producer owns all slots before the published write cursor.
            unsafe { *self.slots[slot].get() = sample };
        }
        self.write
            .store(write.wrapping_add(copied_samples), Ordering::Release);
        self.add_dropped(input.len() - copied_samples);
    }

    fn drain_frames(&self, channels: usize, max_frames: usize, output: &mut Vec<T>) -> usize {
        output.clear();
        if channels == 0 || max_frames == 0 {
            return 0;
        }
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        let available_frames = write.wrapping_sub(read) / channels;
        let frames = available_frames.min(max_frames);
        let samples = frames * channels;

        for offset in 0..samples {
            let slot = (read.wrapping_add(offset)) % self.slots.len();
            // Safety: the consumer only reads slots published before the write cursor.
            output.push(unsafe { *self.slots[slot].get() });
        }
        self.read
            .store(read.wrapping_add(samples), Ordering::Release);
        frames
    }

    fn add_dropped(&self, count: usize) {
        if count != 0 {
            self.dropped.fetch_add(count as u64, Ordering::Relaxed);
        }
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert<T: InputSample>(samples: &[T], channels: usize) -> Vec<f32> {
        let mut mono = Vec::new();
        let mut levels = Vec::new();
        append_mono(samples, channels, &mut mono, &mut levels);
        assert_eq!(mono, levels);
        mono
    }

    #[test]
    fn f32_conversion_clamps_and_sanitizes_non_finite_values() {
        let converted = convert(&[f32::NEG_INFINITY, -1.5, -0.25, f32::NAN, 0.25, 1.5], 1);
        assert_eq!(converted, vec![0.0, -1.0, -0.25, 0.0, 0.25, 1.0]);
    }

    #[test]
    fn i16_conversion_maps_full_range_to_normalized_f32() {
        let converted = convert(&[i16::MIN, -16_384, 0, 16_384, i16::MAX], 1);
        assert_eq!(converted[0], -1.0);
        assert_eq!(converted[1], -0.5);
        assert_eq!(converted[2], 0.0);
        assert!((converted[3] - 16_384.0 / 32_767.0).abs() < f32::EPSILON);
        assert_eq!(converted[4], 1.0);
    }

    #[test]
    fn u16_conversion_maps_midpoint_and_full_range() {
        let converted = convert(&[u16::MIN, 16_384, 32_768, 49_152, u16::MAX], 1);
        assert_eq!(converted[0], -1.0);
        assert_eq!(converted[1], -0.5);
        assert_eq!(converted[2], 0.0);
        assert!((converted[3] - 16_384.0 / 32_767.0).abs() < f32::EPSILON);
        assert_eq!(converted[4], 1.0);
    }

    #[test]
    fn interleaved_channels_are_averaged_per_frame() {
        let stereo = convert(&[i16::MIN, i16::MAX, 16_384, 16_384], 2);
        assert!(stereo[0].abs() < 0.000_1, "mixed value was {}", stereo[0]);
        assert!((stereo[1] - 16_384.0 / 32_767.0).abs() < f32::EPSILON);

        let three_channels = convert(&[1.0_f32, 0.5, -0.5, -1.0, -0.5, 0.5], 3);
        assert!((three_channels[0] - 1.0 / 3.0).abs() < f32::EPSILON);
        assert!((three_channels[1] + 1.0 / 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn incomplete_interleaved_frame_is_ignored() {
        assert_eq!(convert(&[1.0_f32, -1.0, 0.5], 2), vec![0.0]);
        assert!(convert(&[1.0_f32], 0).is_empty());
    }

    #[test]
    fn bounded_ring_preserves_frames_and_counts_overflow() {
        let ring = SampleRing::<i16>::new(4);
        ring.push_frames(&[1, 2, 3, 4, 5, 6], 2);
        assert_eq!(ring.dropped_samples(), 2);

        let mut output = Vec::new();
        assert_eq!(ring.drain_frames(2, 1, &mut output), 1);
        assert_eq!(output, vec![1, 2]);

        ring.push_frames(&[7, 8], 2);
        assert_eq!(ring.drain_frames(2, 8, &mut output), 2);
        assert_eq!(output, vec![3, 4, 7, 8]);
        assert_eq!(ring.dropped_samples(), 2);
    }

    #[test]
    fn bounded_ring_never_publishes_partial_frames() {
        let ring = SampleRing::<u16>::new(6);
        ring.push_frames(&[1, 2, 3, 4], 3);
        assert_eq!(ring.dropped_samples(), 1);

        let mut output = Vec::new();
        assert_eq!(ring.drain_frames(3, 10, &mut output), 1);
        assert_eq!(output, vec![1, 2, 3]);
    }

    #[test]
    fn stream_failures_are_recoverable_audio_device_errors() {
        let unavailable = classify_stream_error(cpal::StreamError::DeviceNotAvailable);
        assert_eq!(unavailable.code, ErrorCode::AudioDevice);

        let backend = classify_stream_error(cpal::StreamError::BackendSpecific {
            err: cpal::BackendSpecificError {
                description: "sensitive sentinel".into(),
            },
        });
        assert_eq!(backend.code, ErrorCode::AudioDevice);
        assert!(!backend.message.contains("sensitive sentinel"));
    }
}
