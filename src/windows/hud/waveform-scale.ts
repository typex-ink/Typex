const VOICE_FLOOR = 0.004;
const SPEECH_CEIL = 0.08;
const ACTIVITY_THRESHOLD = 0.12;
export const PROCESSING_WAVE_CYCLE_MS = 1600;

export function levelToVisualAmplitude(level: number): number {
  if (!Number.isFinite(level)) return 0;
  const clamped = Math.max(0, Math.min(1, level));
  if (clamped <= VOICE_FLOOR) return 0;

  const normalized = Math.min(1, (clamped - VOICE_FLOOR) / (SPEECH_CEIL - VOICE_FLOOR));
  return Math.pow(normalized, 0.55);
}

export function hasVoiceActivity(levels: number[]): boolean {
  return levels.some((level) => levelToVisualAmplitude(level) >= ACTIVITY_THRESHOLD);
}

export function processingWavePhase(elapsedMs: number): number {
  if (!Number.isFinite(elapsedMs) || elapsedMs <= 0) return 0;
  return ((elapsedMs % PROCESSING_WAVE_CYCLE_MS) / PROCESSING_WAVE_CYCLE_MS) * Math.PI * 2;
}
