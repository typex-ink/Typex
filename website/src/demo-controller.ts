export type DemoPhase = "recording" | "processing" | "complete";

export interface DemoSnapshot {
  phase: DemoPhase;
  isPlaying: boolean;
  pausedByUser: boolean;
}

export interface DemoTimer {
  set(callback: () => void, delayMs: number): number;
  clear(id: number): void;
}

export interface DemoControllerOptions {
  reducedMotion: boolean;
  onChange(snapshot: DemoSnapshot): void;
  timer?: DemoTimer;
}

const DURATIONS: Record<DemoPhase, number> = {
  recording: 3600,
  processing: 1700,
  complete: 2600,
};

const NEXT_PHASE: Record<DemoPhase, DemoPhase> = {
  recording: "processing",
  processing: "complete",
  complete: "recording",
};

const browserTimer: DemoTimer = {
  set(callback, delayMs) {
    return window.setTimeout(callback, delayMs);
  },
  clear(id) {
    window.clearTimeout(id);
  },
};

export class DemoController {
  private readonly reducedMotion: boolean;
  private readonly onChange: (snapshot: DemoSnapshot) => void;
  private readonly timer: DemoTimer;
  private phase: DemoPhase;
  private viewportVisible = false;
  private pageVisible = true;
  private pausedByUser = false;
  private timeoutId: number | null = null;

  constructor(options: DemoControllerOptions) {
    this.reducedMotion = options.reducedMotion;
    this.onChange = options.onChange;
    this.timer = options.timer ?? browserTimer;
    this.phase = this.reducedMotion ? "complete" : "recording";
    this.emit();
  }

  setViewportVisible(visible: boolean): void {
    this.viewportVisible = visible;
    this.sync();
  }

  setPageVisible(visible: boolean): void {
    this.pageVisible = visible;
    this.sync();
  }

  toggleUserPaused(): void {
    if (this.reducedMotion) return;
    this.pausedByUser = !this.pausedByUser;
    this.sync();
  }

  dispose(): void {
    this.stopTimer();
  }

  private eligible(): boolean {
    return !this.reducedMotion && this.viewportVisible && this.pageVisible && !this.pausedByUser;
  }

  private sync(): void {
    if (!this.eligible()) {
      this.stopTimer();
      this.emit();
      return;
    }
    if (this.timeoutId === null) {
      this.timeoutId = this.timer.set(() => {
        this.timeoutId = null;
        this.phase = NEXT_PHASE[this.phase];
        this.sync();
      }, DURATIONS[this.phase]);
    }
    this.emit();
  }

  private stopTimer(): void {
    if (this.timeoutId === null) return;
    this.timer.clear(this.timeoutId);
    this.timeoutId = null;
  }

  private emit(): void {
    this.onChange({
      phase: this.phase,
      isPlaying: this.eligible(),
      pausedByUser: this.pausedByUser,
    });
  }
}
