/** A rectangle as fractions (0 to 1) of the page, measured from its top-left corner. */
export interface Region {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Smallest region the picker accepts, as a fraction of the page. */
export const MIN_SIDE = 0.01;

const clamp01 = (n: number): number => Math.min(1, Math.max(0, n));

/** The rectangle spanned by two points given as fractions of the page, in any drag direction. */
export function regionFromDrag(a: { x: number; y: number }, b: { x: number; y: number }): Region | null {
  const x0 = clamp01(Math.min(a.x, b.x));
  const y0 = clamp01(Math.min(a.y, b.y));
  const x1 = clamp01(Math.max(a.x, b.x));
  const y1 = clamp01(Math.max(a.y, b.y));
  const width = x1 - x0;
  const height = y1 - y0;
  return width < MIN_SIDE || height < MIN_SIDE ? null : { x: x0, y: y0, width, height };
}

/** Fraction of the page under a pointer, from the image's on-screen box. */
export function pointToFraction(
  point: { x: number; y: number },
  box: { left: number; top: number; width: number; height: number },
): { x: number; y: number } {
  if (box.width <= 0 || box.height <= 0) return { x: 0, y: 0 };
  return { x: clamp01((point.x - box.left) / box.width), y: clamp01((point.y - box.top) / box.height) };
}

/**
 * Applies a typed change (in percent) to one side of the region and keeps it inside the page.
 * Returns null when the result would be too small to read text from.
 */
export function withPercent(region: Region | null, side: keyof Region, percent: number): Region | null {
  if (!Number.isFinite(percent)) return region;
  const base = region ?? { x: 0, y: 0, width: 1, height: 0.1 };
  const value = clamp01(percent / 100);
  const next = { ...base, [side]: value };
  next.x = Math.min(next.x, 1 - MIN_SIDE);
  next.y = Math.min(next.y, 1 - MIN_SIDE);
  next.width = Math.min(Math.max(next.width, MIN_SIDE), 1 - next.x);
  next.height = Math.min(Math.max(next.height, MIN_SIDE), 1 - next.y);
  return next;
}

/** Percent with one decimal, for the numeric fields. */
export function toPercent(fraction: number): number {
  return Math.round(fraction * 1000) / 10;
}
