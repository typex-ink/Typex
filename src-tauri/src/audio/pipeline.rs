//! 重采样（rubato → 16 kHz mono）+ 电平计算 + WAV 编码（07 §7.4）。
//! VAD 静音裁剪与长录音切片在 CP-1.9 加入。

use crate::error::{ErrorCode, Result, TypexError};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

pub const TARGET_RATE: u32 = 16_000;

/// 把电平窗口压缩为 n 段 RMS（HUD 波形柱数据）。
pub fn rms_levels(samples: &[f32], n: usize) -> Vec<f32> {
    if samples.is_empty() || n == 0 {
        return vec![0.0; n];
    }
    let chunk = samples.len().div_ceil(n);
    samples
        .chunks(chunk)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect()
}

/// 任意采样率 mono f32 → 16 kHz 16-bit PCM WAV 字节。
pub fn to_wav_16k_mono(samples: &[f32], src_rate: u32) -> Result<Vec<u8>> {
    let resampled = if src_rate == TARGET_RATE {
        samples.to_vec()
    } else {
        resample(samples, src_rate, TARGET_RATE)?
    };
    encode_wav(&resampled, TARGET_RATE)
}

/// 录音后处理：重采样 → VAD 首尾静音裁剪 → WAV（07 §7.4）。
/// 返回 (wav, 有效时长 ms)；全静音时 wav 为空录音（0 采样）。
pub fn finalize_recording(samples: &[f32], src_rate: u32) -> Result<(Vec<u8>, u64)> {
    let resampled = if src_rate == TARGET_RATE {
        samples.to_vec()
    } else {
        resample(samples, src_rate, TARGET_RATE)?
    };
    let (start, end) = super::vad::trim_silence(&resampled, 150);
    let trimmed = &resampled[start..end];
    let duration_ms = (trimmed.len() as u64 * 1000) / TARGET_RATE as u64;
    Ok((encode_wav(trimmed, TARGET_RATE)?, duration_ms))
}

fn resample(input: &[f32], from: u32, to: u32) -> Result<Vec<f32>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    const CHUNK: usize = 1024;
    let mut resampler = SincFixedIn::<f32>::new(to as f64 / from as f64, 2.0, params, CHUNK, 1)
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("重采样器创建失败: {e}")))?;
    let mut out = Vec::with_capacity(input.len() * to as usize / from as usize + CHUNK);
    let mut pos = 0;
    while pos + CHUNK <= input.len() {
        let frames = resampler
            .process(&[&input[pos..pos + CHUNK]], None)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("重采样失败: {e}")))?;
        out.extend_from_slice(&frames[0]);
        pos += CHUNK;
    }
    // 尾块（不足一个 chunk）
    if pos < input.len() {
        let frames = resampler
            .process_partial(Some(&[&input[pos..]]), None)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("重采样失败: {e}")))?;
        out.extend_from_slice(&frames[0]);
    }
    Ok(out)
}

fn encode_wav(samples: &[f32], rate: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("WAV 编码失败: {e}")))?;
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer
                .write_sample(v)
                .map_err(|e| TypexError::new(ErrorCode::Internal, format!("WAV 写入失败: {e}")))?;
        }
        writer
            .finalize()
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("WAV 完成失败: {e}")))?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn rms_levels_shape_and_loudness() {
        let quiet = vec![0.01f32; 1600];
        let loud = sine(440.0, 16_000, 0.1);
        let lq = rms_levels(&quiet, 12);
        let ll = rms_levels(&loud, 12);
        assert_eq!(lq.len(), 12);
        assert!(ll.iter().sum::<f32>() > lq.iter().sum::<f32>());
    }

    #[test]
    fn resample_48k_to_16k_length_and_frequency() {
        let input = sine(440.0, 48_000, 1.0);
        let out = resample(&input, 48_000, 16_000).unwrap();
        // 输出长度约 1/3（sinc 边缘允许 ±5%）
        let expect = input.len() / 3;
        assert!(
            (out.len() as i64 - expect as i64).unsigned_abs() < (expect / 20) as u64,
            "len {} vs expect {expect}",
            out.len()
        );
        // 频率不漂移：过零率 ≈ 2 × 440 次/秒
        let zc = out
            .windows(2)
            .filter(|w| w[0].signum() != w[1].signum())
            .count();
        let dur = out.len() as f32 / 16_000.0;
        let freq = zc as f32 / dur / 2.0;
        assert!((freq - 440.0).abs() < 15.0, "measured freq {freq}");
    }

    #[test]
    fn wav_output_is_valid_16k_mono() {
        let wav = to_wav_16k_mono(&sine(440.0, 44_100, 0.5), 44_100).unwrap();
        let reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);
    }

    #[test]
    fn empty_input_yields_valid_empty_wav() {
        let wav = to_wav_16k_mono(&[], 48_000).unwrap();
        let reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
        assert_eq!(reader.len(), 0);
    }
}
