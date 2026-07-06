import { describe, expect, it } from "vitest";
import { hasVoiceActivity, levelToVisualAmplitude } from "./waveform-scale";

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
});
