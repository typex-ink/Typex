import { describe, expect, it } from "vitest";
import { DemoController, type DemoSnapshot, type DemoTimer } from "./demo-controller";

class ManualTimer implements DemoTimer {
  nextId = 1;
  callbacks = new Map<number, () => void>();
  cleared: number[] = [];

  set(callback: () => void): number {
    const id = this.nextId++;
    this.callbacks.set(id, callback);
    return id;
  }

  clear(id: number): void {
    this.cleared.push(id);
    this.callbacks.delete(id);
  }

  runNext(): void {
    const entry = this.callbacks.entries().next().value as [number, () => void] | undefined;
    if (!entry) throw new Error("No scheduled callback");
    this.callbacks.delete(entry[0]);
    entry[1]();
  }
}

describe("demo controller", () => {
  it("uses a static completed state for reduced motion without scheduling", () => {
    const timer = new ManualTimer();
    const snapshots: DemoSnapshot[] = [];
    const controller = new DemoController({
      reducedMotion: true,
      timer,
      onChange: (snapshot) => snapshots.push(snapshot),
    });

    controller.setViewportVisible(true);
    controller.setPageVisible(true);

    expect(snapshots.at(-1)).toEqual({
      phase: "complete",
      isPlaying: false,
      pausedByUser: false,
    });
    expect(timer.callbacks.size).toBe(0);
  });

  it("cycles through recording, processing, and completed phases", () => {
    const timer = new ManualTimer();
    const snapshots: DemoSnapshot[] = [];
    const controller = new DemoController({
      reducedMotion: false,
      timer,
      onChange: (snapshot) => snapshots.push(snapshot),
    });

    controller.setViewportVisible(true);
    expect(snapshots.at(-1)?.phase).toBe("recording");
    expect(snapshots.at(-1)?.isPlaying).toBe(true);

    timer.runNext();
    expect(snapshots.at(-1)?.phase).toBe("processing");
    timer.runNext();
    expect(snapshots.at(-1)?.phase).toBe("complete");
    timer.runNext();
    expect(snapshots.at(-1)?.phase).toBe("recording");
  });

  it("stops when hidden, offscreen, or manually paused and resumes on demand", () => {
    const timer = new ManualTimer();
    const snapshots: DemoSnapshot[] = [];
    const controller = new DemoController({
      reducedMotion: false,
      timer,
      onChange: (snapshot) => snapshots.push(snapshot),
    });

    controller.setViewportVisible(true);
    expect(timer.callbacks.size).toBe(1);
    controller.setPageVisible(false);
    expect(timer.callbacks.size).toBe(0);
    expect(snapshots.at(-1)?.isPlaying).toBe(false);

    controller.setPageVisible(true);
    expect(timer.callbacks.size).toBe(1);
    controller.setViewportVisible(false);
    expect(timer.callbacks.size).toBe(0);

    controller.setViewportVisible(true);
    controller.toggleUserPaused();
    expect(timer.callbacks.size).toBe(0);
    expect(snapshots.at(-1)?.pausedByUser).toBe(true);

    controller.toggleUserPaused();
    expect(timer.callbacks.size).toBe(1);
    expect(snapshots.at(-1)?.isPlaying).toBe(true);
  });
});
