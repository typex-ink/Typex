//! Configurable VAD for silence trimming and long-recording chunk boundaries.

use crate::settings::schema::{VadMode, VadSettings};
use silero_vad_crs::{SileroVad, SileroVadError, TimestampConfig};
use std::ops::Range;

const FRAME_MS: usize = 30;
pub const SAMPLE_RATE: usize = 16_000;
const FRAME_LEN: usize = SAMPLE_RATE * FRAME_MS / 1000;
const NEURAL_WINDOW_SAMPLES: usize = 512;
const MIN_SPEECH_MS: usize = 90;
const MIN_SILENCE_MS: usize = 100;
const LEADING_PAD_MS: usize = 300;
const TRAILING_PAD_MS: usize = 150;
const WEAK_SIGNAL_RATIO: f32 = 0.10;
const WEAK_SIGNAL_FLOOR: f32 = 0.000_1;

fn frame_rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32).sqrt()
}

fn energy_regions(samples: &[f32], threshold: f32) -> Vec<Range<usize>> {
    let mut regions = Vec::<Range<usize>>::new();
    for (index, frame) in samples.chunks(FRAME_LEN).enumerate() {
        if frame_rms(frame) <= threshold {
            continue;
        }
        let start = index * FRAME_LEN;
        let end = (start + frame.len()).min(samples.len());
        if let Some(previous) = regions.last_mut()
            && previous.end == start
        {
            previous.end = end;
        } else {
            regions.push(start..end);
        }
    }
    regions
}

fn neural_regions(samples: &[f32], threshold: f32) -> Result<Vec<Range<usize>>, SileroVadError> {
    let mut model = SileroVad::new()?;
    let probabilities = model.forward_audio(samples)?;
    Ok(neural_regions_from_probs(
        &probabilities,
        samples.len(),
        threshold,
    ))
}

fn neural_regions_from_probs(
    probabilities: &[f32],
    audio_len: usize,
    threshold: f32,
) -> Vec<Range<usize>> {
    silero_vad_crs::get_timestamps_from_probs_with_config(
        probabilities,
        audio_len,
        TimestampConfig {
            sampling_rate: SAMPLE_RATE,
            threshold,
            min_speech_duration_ms: MIN_SPEECH_MS,
            min_silence_duration_ms: MIN_SILENCE_MS,
            speech_pad_ms: 0,
            neg_threshold: Some((threshold - 0.15).max(0.01)),
            window_size_samples: NEURAL_WINDOW_SAMPLES,
            ..TimestampConfig::default()
        },
    )
    .into_iter()
    .map(|timestamp| timestamp.start..timestamp.end)
    .collect()
}

fn speech_regions_with_neural<F>(
    samples: &[f32],
    settings: VadSettings,
    neural: F,
) -> Vec<Range<usize>>
where
    F: FnOnce(&[f32], f32) -> Result<Vec<Range<usize>>, SileroVadError>,
{
    match settings.mode {
        VadMode::Energy => energy_regions(samples, settings.energy_threshold),
        VadMode::Neural => match neural(samples, settings.neural_threshold) {
            Ok(regions) => regions,
            Err(error) => {
                tracing::warn!(
                    error_kind = ?error,
                    "Silero VAD failed; falling back to configured energy VAD"
                );
                energy_regions(samples, settings.energy_threshold)
            }
        },
    }
}

fn speech_regions(samples: &[f32], settings: VadSettings) -> Vec<Range<usize>> {
    speech_regions_with_neural(samples, settings, neural_regions)
}

fn weak_signal_threshold(settings: VadSettings) -> f32 {
    (settings.energy_threshold * WEAK_SIGNAL_RATIO).max(WEAK_SIGNAL_FLOOR)
}

fn has_continuous_weak_signal(samples: &[f32], settings: VadSettings) -> bool {
    let threshold = weak_signal_threshold(settings);
    let required = SAMPLE_RATE * MIN_SPEECH_MS / 1000;
    let mut continuous = 0usize;
    for frame in samples.chunks(FRAME_LEN) {
        if frame_rms(frame) > threshold {
            continuous += frame.len();
            if continuous >= required {
                return true;
            }
        } else {
            continuous = 0;
        }
    }
    false
}

/// Return the `[start, end)` range submitted to STT.
///
/// Detected speech receives asymmetric 300/150 ms padding. If no VAD path
/// detects speech, a continuous 90 ms weak signal keeps the original audio;
/// true silence returns `(0, 0)`.
pub fn trim_silence(samples: &[f32], settings: VadSettings) -> (usize, usize) {
    trim_silence_with_neural(samples, settings, neural_regions)
}

fn trim_silence_with_neural<F>(samples: &[f32], settings: VadSettings, neural: F) -> (usize, usize)
where
    F: FnOnce(&[f32], f32) -> Result<Vec<Range<usize>>, SileroVadError>,
{
    let regions = speech_regions_with_neural(samples, settings, neural);
    match (regions.first(), regions.last()) {
        (Some(first), Some(last)) => {
            tracing::debug!(first_speech_frame_ms = first.start as u64 * 1000 / SAMPLE_RATE as u64);
            let leading_pad = SAMPLE_RATE * LEADING_PAD_MS / 1000;
            let trailing_pad = SAMPLE_RATE * TRAILING_PAD_MS / 1000;
            (
                first.start.saturating_sub(leading_pad),
                (last.end + trailing_pad).min(samples.len()),
            )
        }
        _ if has_continuous_weak_signal(samples, settings) => (0, samples.len()),
        _ => (0, 0),
    }
}

/// Split long audio at the nearest VAD-silent interval before each hard limit.
pub fn split_at_silence(
    samples: &[f32],
    max_samples: usize,
    settings: VadSettings,
) -> Vec<(usize, usize)> {
    if samples.len() <= max_samples || max_samples == 0 {
        return vec![(0, samples.len())];
    }

    let mut regions = speech_regions(samples, settings);
    if regions.is_empty() && has_continuous_weak_signal(samples, settings) {
        regions = energy_regions(samples, weak_signal_threshold(settings));
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while samples.len() - start > max_samples {
        let hard_end = start + max_samples;
        let search_floor = start + max_samples / 2;
        let mut cut = hard_end;
        let mut frame_end = hard_end;
        while frame_end > search_floor + FRAME_LEN {
            let frame_start = frame_end - FRAME_LEN;
            let overlaps_speech = regions
                .iter()
                .any(|speech| speech.start < frame_end && speech.end > frame_start);
            if !overlaps_speech {
                cut = frame_start + FRAME_LEN / 2;
                break;
            }
            frame_end -= FRAME_LEN;
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

    fn energy_settings() -> VadSettings {
        VadSettings {
            mode: VadMode::Energy,
            ..VadSettings::default()
        }
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (SAMPLE_RATE as f32 * seconds) as usize]
    }

    fn tone(seconds: f32, amplitude: f32) -> Vec<f32> {
        (0..(SAMPLE_RATE as f32 * seconds) as usize)
            .map(|index| {
                (2.0 * std::f32::consts::PI * 200.0 * index as f32 / SAMPLE_RATE as f32).sin()
                    * amplitude
            })
            .collect()
    }

    #[test]
    fn energy_vad_uses_asymmetric_padding() {
        let mut audio = vec![0.0; FRAME_LEN * 40];
        let speech_start = audio.len();
        audio.extend(tone(FRAME_LEN as f32 * 60.0 / SAMPLE_RATE as f32, 0.3));
        let speech_end = audio.len();
        audio.extend(vec![0.0; FRAME_LEN * 40]);

        let (start, end) = trim_silence(&audio, energy_settings());
        assert_eq!(start, speech_start - SAMPLE_RATE * 300 / 1000);
        assert_eq!(end, speech_end + SAMPLE_RATE * 150 / 1000);
    }

    #[test]
    fn all_silence_yields_empty() {
        assert_eq!(trim_silence(&silence(3.0), energy_settings()), (0, 0));
        assert_eq!(trim_silence(&silence(1.0), VadSettings::default()), (0, 0));
    }

    #[test]
    fn continuous_weak_signal_keeps_untrimmed_audio() {
        let mut audio = silence(0.3);
        audio.extend(tone(0.09, 0.004));
        audio.extend(silence(0.3));
        assert_eq!(trim_silence(&audio, energy_settings()), (0, audio.len()));
    }

    #[test]
    fn sub_floor_signal_is_still_silence() {
        let audio = tone(0.5, WEAK_SIGNAL_FLOOR / 4.0);
        assert_eq!(trim_silence(&audio, energy_settings()), (0, 0));
    }

    #[test]
    fn neural_probabilities_use_configured_hysteresis_and_minimum_duration() {
        let probabilities = [0.0, 0.8, 0.8, 0.8, 0.2, 0.2, 0.2, 0.2, 0.2];
        let regions = neural_regions_from_probs(
            &probabilities,
            probabilities.len() * NEURAL_WINDOW_SAMPLES,
            0.5,
        );
        assert_eq!(
            regions,
            vec![NEURAL_WINDOW_SAMPLES..4 * NEURAL_WINDOW_SAMPLES]
        );
    }

    #[test]
    fn neural_failure_falls_back_to_configured_energy_threshold() {
        let settings = VadSettings {
            energy_threshold: 0.02,
            ..VadSettings::default()
        };
        let mut audio = vec![0.0; FRAME_LEN * 20];
        let speech_start = audio.len();
        audio.extend(tone(0.2, 0.1));

        let (start, end) =
            trim_silence_with_neural(&audio, settings, |_, _| Err(SileroVadError::CreateFailed));
        assert_eq!(start, speech_start - SAMPLE_RATE * 300 / 1000);
        assert_eq!(end, audio.len());
    }

    #[test]
    fn short_audio_is_not_split() {
        let audio = tone(2.0, 0.3);
        let chunks = split_at_silence(&audio, SAMPLE_RATE * 60, energy_settings());
        assert_eq!(chunks, vec![(0, audio.len())]);
    }

    #[test]
    fn split_lands_in_silence_gap() {
        let mut audio = tone(30.0, 0.3);
        let gap_start = audio.len();
        audio.extend(silence(1.0));
        let gap_end = audio.len();
        audio.extend(tone(30.0, 0.3));

        let chunks = split_at_silence(&audio, SAMPLE_RATE * 40, energy_settings());
        assert_eq!(chunks.len(), 2);
        let cut = chunks[0].1;
        assert!(cut >= gap_start && cut <= gap_end);
        assert_eq!(chunks[0].1, chunks[1].0);
        assert_eq!(chunks[1].1, audio.len());
    }

    #[test]
    fn continuous_speech_is_force_split() {
        let audio = tone(90.0, 0.3);
        let chunks = split_at_silence(&audio, SAMPLE_RATE * 40, energy_settings());
        assert!(chunks.len() >= 3);
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
        assert!(
            chunks
                .iter()
                .all(|(start, end)| end - start <= SAMPLE_RATE * 40)
        );
    }
}
