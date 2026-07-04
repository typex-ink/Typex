//! 提示音（04 §6）：三枚合成音，皆 < 150 ms、木琴系音色、可关。
//! 开始 = 上行双音「叮·咚」；成功 = 单音短「嗒」；错误 = 低频闷「噗」。
//! 不依赖音频文件——正弦 + 指数衰减合成，体积零成本。

use rodio::{OutputStreamBuilder, Sink, buffer::SamplesBuffer};

#[derive(Debug, Clone, Copy)]
pub enum ChimeKind {
    Start,
    Success,
    Error,
}

const RATE: u32 = 44_100;

/// 木琴系音色：正弦基波 + 三次谐波少量 + 指数衰减包络。
fn strike(freq: f32, dur_ms: u32, gain: f32) -> Vec<f32> {
    let n = (RATE * dur_ms / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (-t * 18.0).exp();
            let wave = (2.0 * std::f32::consts::PI * freq * t).sin()
                + 0.25 * (2.0 * std::f32::consts::PI * freq * 3.0 * t).sin();
            wave * env * gain
        })
        .collect()
}

fn samples_of(kind: ChimeKind, volume: f32) -> Vec<f32> {
    let g = volume.clamp(0.0, 1.0) * 0.5;
    match kind {
        ChimeKind::Start => {
            // 上行双音：G5 → C6，各 70ms
            let mut s = strike(784.0, 70, g);
            s.extend(strike(1046.5, 80, g));
            s
        }
        ChimeKind::Success => strike(1318.5, 90, g), // E6 短「嗒」
        ChimeKind::Error => strike(196.0, 140, g * 1.2), // G3 低频闷「噗」
    }
}

/// 播放提示音（异步 fire-and-forget；播放失败静默——提示音不值得报错）。
pub fn play(kind: ChimeKind, volume: f32) {
    std::thread::spawn(move || {
        let Ok(stream) = OutputStreamBuilder::open_default_stream() else {
            return;
        };
        let sink = Sink::connect_new(stream.mixer());
        let samples = samples_of(kind, volume);
        sink.append(SamplesBuffer::new(1, RATE, samples));
        sink.sleep_until_end();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_chimes_under_150ms() {
        for kind in [ChimeKind::Start, ChimeKind::Success, ChimeKind::Error] {
            let s = samples_of(kind, 1.0);
            let ms = s.len() as f32 / RATE as f32 * 1000.0;
            assert!(ms <= 150.0, "{kind:?} = {ms}ms");
        }
    }

    #[test]
    fn samples_within_unit_range() {
        for kind in [ChimeKind::Start, ChimeKind::Success, ChimeKind::Error] {
            let s = samples_of(kind, 1.0);
            assert!(s.iter().all(|v| v.abs() <= 1.0));
            assert!(s.iter().any(|v| v.abs() > 0.05), "{kind:?} 无声");
        }
    }
}
