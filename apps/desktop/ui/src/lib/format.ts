import type { PageInfo } from './ipc/generated';

const UNITS = ['B', 'KB', 'MB', 'GB'] as const;

/** Formats a byte count with binary units, matching how the core measures sizes. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

/** A run of pages that share one size. */
export interface PageSizeGroup {
  count: number;
  width: number;
  height: number;
  firstPage: number;
}

/** Groups pages by their size in whole points, in order of first appearance. */
export function groupPageSizes(pages: PageInfo[]): PageSizeGroup[] {
  const groups = new Map<string, PageSizeGroup>();
  for (const page of pages) {
    const width = Math.round(page.width_pt);
    const height = Math.round(page.height_pt);
    const key = `${width}x${height}`;
    const existing = groups.get(key);
    if (existing) existing.count += 1;
    else groups.set(key, { count: 1, width, height, firstPage: page.number });
  }
  return [...groups.values()];
}
