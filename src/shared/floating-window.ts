export interface LogicalRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LogicalSizeLike {
  width: number;
  height: number;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

export function fitRectInWorkArea(
  rect: LogicalRect,
  workArea: LogicalRect,
  margin: number,
): { x: number; y: number } {
  const minX = workArea.x + margin;
  const maxX = workArea.x + workArea.width - rect.width - margin;
  const minY = workArea.y + margin;
  const maxY = workArea.y + workArea.height - rect.height - margin;
  return {
    x: clamp(rect.x, minX, maxX),
    y: clamp(rect.y, minY, maxY),
  };
}

export function bottomCenteredRect(
  size: LogicalSizeLike,
  workArea: LogicalRect,
  bottomGap: number,
): { x: number; y: number } {
  return fitRectInWorkArea(
    {
      x: workArea.x + (workArea.width - size.width) / 2,
      y: workArea.y + workArea.height - size.height - bottomGap,
      width: size.width,
      height: size.height,
    },
    workArea,
    0,
  );
}
