import { describe, expect, it } from 'vitest';
import { formatBytes, groupPageSizes } from './format';

describe('formatBytes', () => {
  it('uses binary units with sensible precision', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1,023 B');
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(10 * 1024 * 1024)).toBe('10.0 MB');
    expect(formatBytes(200 * 1024 * 1024)).toBe('200 MB');
    expect(formatBytes(5 * 1024 ** 4)).toBe('5,120 GB');
  });

  it('follows the language for separators', () => {
    expect(formatBytes(1536, 'de')).toBe('1,5 KB');
    expect(formatBytes(5 * 1024 ** 4, 'de')).toBe('5.120 GB');
  });
});

describe('groupPageSizes', () => {
  it('groups pages by rounded size in order of first appearance', () => {
    const page = (number: number, w: number, h: number) => ({
      number,
      width_pt: w,
      height_pt: h,
      rotation: 0,
    });
    const groups = groupPageSizes([
      page(1, 612, 792),
      page(2, 595.3, 841.9),
      page(3, 612.2, 791.8),
    ]);
    expect(groups).toEqual([
      { count: 2, width: 612, height: 792, firstPage: 1 },
      { count: 1, width: 595, height: 842, firstPage: 2 },
    ]);
  });
});
