import { describe, expect, it } from 'vitest';
import { MIN_SIDE, pointToFraction, regionFromDrag, toPercent, withPercent } from './region';

describe('region geometry', () => {
  it('builds the same rectangle whichever way the user drags', () => {
    const a = regionFromDrag({ x: 0.2, y: 0.1 }, { x: 0.6, y: 0.3 });
    const b = regionFromDrag({ x: 0.6, y: 0.3 }, { x: 0.2, y: 0.1 });
    expect(a).toEqual(b);
    expect(a?.x).toBeCloseTo(0.2);
    expect(a?.width).toBeCloseTo(0.4);
    expect(a?.height).toBeCloseTo(0.2);
  });

  it('ignores accidental clicks and keeps the region inside the page', () => {
    expect(regionFromDrag({ x: 0.5, y: 0.5 }, { x: 0.505, y: 0.9 })).toBeNull();
    expect(regionFromDrag({ x: -1, y: -1 }, { x: 2, y: 2 })).toEqual({ x: 0, y: 0, width: 1, height: 1 });
  });

  it('turns pointer positions into page fractions and clamps outside the image', () => {
    const box = { left: 100, top: 50, width: 200, height: 400 };
    expect(pointToFraction({ x: 200, y: 250 }, box)).toEqual({ x: 0.5, y: 0.5 });
    expect(pointToFraction({ x: 0, y: 1000 }, box)).toEqual({ x: 0, y: 1 });
    expect(pointToFraction({ x: 5, y: 5 }, { left: 0, top: 0, width: 0, height: 0 })).toEqual({ x: 0, y: 0 });
  });

  it('applies typed percentages without leaving the page', () => {
    const r = withPercent({ x: 0.5, y: 0.1, width: 0.4, height: 0.2 }, 'width', 80);
    expect(r?.width).toBeCloseTo(0.5);
    expect(withPercent(null, 'x', 30)).toMatchObject({ x: 0.3 });
    expect(withPercent(null, 'height', 0)?.height).toBeCloseTo(MIN_SIDE);
    const same = { x: 0, y: 0, width: 0.5, height: 0.5 };
    expect(withPercent(same, 'x', Number.NaN)).toBe(same);
  });

  it('shows percentages with one decimal', () => {
    expect(toPercent(0.12345)).toBe(12.3);
    expect(toPercent(1)).toBe(100);
  });
});
