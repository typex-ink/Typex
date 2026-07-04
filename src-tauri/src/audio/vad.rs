//! VAD：静音裁剪 + 长录音切片（07 §7.4）。
//!
//! v1 先用能量阈值 VAD（无额外依赖）；模块边界已按可替换设计——
//! Silero（vad-rs/onnx）后续可直接替换 `is_speech_frame` 的实现（见 ROADMAP CP-1.9 注）。

const FRAME_MS: usize = 30;
const SAMPLE_RATE: usize = 16_000;
const FRAME_LEN: usize = SAMPLE_RATE * FRAME_MS / 1000; // 480

/// 能量阈值（相对满幅 RMS）；实测人声 RMS 远高于环境噪声
const ENERGY_THRESHOLD: f32 = 0.010;

fn frame_rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

fn is_speech_frame(frame: &[f32]) -> bool {
    frame_rms(frame) > ENERGY_THRESHOLD
}

/// 首尾静音裁剪：返回 [start, end) 采样区间。前后各保留 pad_ms 缓冲。
pub fn trim_silence(samples: &[f32], pad_ms: usize) -> (usize, usize) {
    let pad = SAMPLE_RATE * pad_ms / 1000;
    let n_frames = samples.len() / FRAME_LEN;
    let mut first = None;
    let mut last = None;
    for i in 0..n_frames {
        let frame = &samples[i * FRAME_LEN..(i + 1) * FRAME_LEN];
        if is_speech_frame(frame) {
            if first.is_none() {
                first = Some(i);
            }
            last = Some(i);
        }
    }
    match (first, last) {
        (Some(f), Some(l)) => {
            let start = (f * FRAME_LEN).saturating_sub(pad);
            let end = ((l + 1) * FRAME_LEN + pad).min(samples.len());
            (start, end)
        }
        _ => (0, 0), // 全静音
    }
}

/// 长音频切片：在静音帧处分割，每片 ≤ max_samples；无静音处则强制切。
/// 返回各片的 [start, end) 区间（相对原始采样）。
pub fn split_at_silence(samples: &[f32], max_samples: usize) -> Vec<(usize, usize)> {
    if samples.len() <= max_samples {
        return vec![(0, samples.len())];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while samples.len() - start > max_samples {
        let hard_end = start + max_samples;
        // 从 hard_end 往回找最近的静音帧（最多回看 50%——尽量切在静音处）
        let search_floor = start + max_samples / 2;
        let mut cut = hard_end;
        let mut i = hard_end;
        while i > search_floor + FRAME_LEN {
            let f_start = i - FRAME_LEN;
            if !is_speech_frame(&samples[f_start..i]) {
                cut = f_start + FRAME_LEN / 2; // 切在静音帧中部
                break;
            }
            i -= FRAME_LEN;
        }
        chunks.push((start, cut));
        start = cut;
    }
    chunks.push((start, samples.len()));
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (SAMPLE_RATE as f32 * secs) as usize]
    }

    fn speech(secs: f32) -> Vec<f32> {
        (0..(SAMPLE_RATE as f32 * secs) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 200.0 * i as f32 / SAMPLE_RATE as f32).sin() * 0.3)
            .collect()
    }

    #[test]
    fn trim_removes_leading_and_trailing_silence() {
        let mut audio = silence(1.0);
        let speech_start = audio.len();
        audio.extend(speech(2.0));
        let speech_end = audio.len();
        audio.extend(silence(1.5));

        let (start, end) = trim_silence(&audio, 100);
        // 切点落在语音段边界 ±150ms 内
        let tol = SAMPLE_RATE * 150 / 1000;
        assert!(start.abs_diff(speech_start) < tol, "start {start} vs {speech_start}");
        assert!(end.abs_diff(speech_end) < tol, "end {end} vs {speech_end}");
    }

    #[test]
    fn all_silence_yields_empty() {
        assert_eq!(trim_silence(&silence(3.0), 100), (0, 0));
    }

    #[test]
    fn short_audio_not_split() {
        let audio = speech(2.0);
        let chunks = split_at_silence(&audio, SAMPLE_RATE * 60);
        assert_eq!(chunks, vec![(0, audio.len())]);
    }

    #[test]
    fn split_lands_in_silence_gap() {
        // 30s 语音 + 1s 静音 + 30s 语音；上限 40s → 切点应落在静音段
        let mut audio = speech(30.0);
        let gap_start = audio.len();
        audio.extend(silence(1.0));
        let gap_end = audio.len();
        audio.extend(speech(30.0));

        let chunks = split_at_silence(&audio, SAMPLE_RATE * 40);
        assert_eq!(chunks.len(), 2);
        let cut = chunks[0].1;
        assert!(cut >= gap_start && cut <= gap_end, "cut {cut} not in [{gap_start},{gap_end}]");
        // 无缝衔接
        assert_eq!(chunks[0].1, chunks[1].0);
        assert_eq!(chunks[1].1, audio.len());
    }

    #[test]
    fn continuous_speech_force_split() {
        let audio = speech(90.0);
        let chunks = split_at_silence(&audio, SAMPLE_RATE * 40);
        assert!(chunks.len() >= 3);
        for w in chunks.windows(2) {
            assert_eq!(w[0].1, w[1].0);
        }
        assert!(chunks.iter().all(|(s, e)| e - s <= SAMPLE_RATE * 40));
    }
}
