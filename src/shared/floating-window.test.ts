import { describe, expect, it } from "vitest";
import { bottomCenteredRect, fitRectInWorkArea } from "./floating-window";

describe("floating window geometry", () => {
  const workArea = { x: 0, y: 24, width: 1440, height: 876 };

  it("keeps a grown assistant panel inside the work area", () => {
    const pos = fitRectInWorkArea(
      { x: 440, y: 760, width: 560, height: 360 },
      workArea,
      12,
    );
    expect(pos).toEqual({ x: 440, y: 528 });
  });

  it("centers HUD at the bottom with the requested gap", () => {
    const pos = bottomCenteredRect({ width: 420, height: 44 }, workArea, 48);
    expect(pos).toEqual({ x: 510, y: 808 });
  });

  it("clamps oversized floating windows to the top-left margin", () => {
    const pos = fitRectInWorkArea(
      { x: 300, y: 300, width: 2000, height: 1200 },
      workArea,
      12,
    );
    expect(pos).toEqual({ x: 12, y: 36 });
  });
});
