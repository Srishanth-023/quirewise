import { ipc } from '../../ipc/client';

/** A rendered page ready to show in an `<img>`. */
export interface Thumb {
  url: string;
  width: number;
  height: number;
  pageCount: number;
}

const MAX_CONCURRENT = 3;
const MAX_ENTRIES = 300;

const cache = new Map<string, Promise<Thumb>>();
const waiting: (() => void)[] = [];
let active = 0;

function key(path: string, hasPassword: boolean, page: number, edge: number): string {
  // The password itself is never part of a key.
  return `${path}|${hasPassword ? 1 : 0}|${page}|${edge}`;
}

async function slot<T>(work: () => Promise<T>): Promise<T> {
  if (active >= MAX_CONCURRENT) await new Promise<void>((resolve) => waiting.push(resolve));
  active += 1;
  try {
    return await work();
  } finally {
    active -= 1;
    waiting.shift()?.();
  }
}

/**
 * Renders one page through the worker, at most three at a time, and remembers the result.
 * Failures are not cached so a retry after unlocking the file works.
 */
export function thumbnail(
  path: string,
  password: string | null,
  page: number,
  edge: number,
): Promise<Thumb> {
  const k = key(path, password !== null, page, edge);
  const hit = cache.get(k);
  if (hit) {
    // Re-insert to mark as recently used.
    cache.delete(k);
    cache.set(k, hit);
    return hit;
  }
  const pending = slot(async () => {
    const t = await ipc.renderThumbnail(path, password, page, edge);
    return {
      url: `data:image/png;base64,${t.png}`,
      width: t.width,
      height: t.height,
      pageCount: t.page_count,
    };
  });
  cache.set(k, pending);
  pending.catch(() => cache.delete(k));
  while (cache.size > MAX_ENTRIES) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
  return pending;
}

/** Forgets every cached page of `path`, for example after the file was unlocked. */
export function forgetThumbnails(path: string): void {
  for (const k of [...cache.keys()]) {
    if (k.startsWith(`${path}|`)) cache.delete(k);
  }
}
