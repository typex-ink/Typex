import { describe, expect, it } from "vitest";
import {
  PROCESSING_WAVE_CYCLE_MS,
  hasVoiceActivity,
  levelToVisualAmplitude,
  processingWavePhase,
} from "../../shared/waveform-scale";

describe("HUD waveform scale", () => {
  it("keeps silence at the visual floor", () => {
    expect(levelToVisualAmplitude(0)).toBe(0);
    expect(levelToVisualAmplitude(0.003)).toBe(0);
    expect(hasVoiceActivity([0, 0.002, 0.003])).toBe(false);
  });

  it("makes quiet speech visibly move", () => {
    expect(levelToVisualAmplitude(0.01)).toBeGreaterThan(0.2);
    expect(hasVoiceActivity([0, 0.01, 0])).toBe(true);
  });

  it("compresses loud samples without exceeding the canvas range", () => {
    expect(levelToVisualAmplitude(0.04)).toBeGreaterThan(levelToVisualAmplitude(0.01));
    expect(levelToVisualAmplitude(0.5)).toBe(1);
  });

  it("derives the processing phase from elapsed time with a fixed 1.6s cycle", () => {
    expect(processingWavePhase(0)).toBe(0);
    expect(processingWavePhase(PROCESSING_WAVE_CYCLE_MS / 4)).toBeCloseTo(Math.PI / 2);
    expect(processingWavePhase(PROCESSING_WAVE_CYCLE_MS / 2)).toBeCloseTo(Math.PI);
    expect(processingWavePhase(PROCESSING_WAVE_CYCLE_MS)).toBeCloseTo(0);
  });
});
