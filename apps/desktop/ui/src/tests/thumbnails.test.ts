import { beforeEach, describe, expect, it, vi } from 'vitest';

const render = vi.hoisted(() => vi.fn());
vi.mock('../lib/ipc/client', () => ({
  ipc: { renderThumbnail: render },
  toAppError: (e: unknown) => e,
}));

import { forgetThumbnails, thumbnail } from '../lib/components/Thumbnail/thumbnails';

const png = { width: 10, height: 14, page_count: 3, png: 'AAAA' };

beforeEach(() => {
  forgetThumbnails('C:\\a.pdf');
  render.mockReset();
});

describe('thumbnail loader', () => {
  it('turns the PNG into a data URL and caches the result', async () => {
    render.mockResolvedValue(png);
    const first = await thumbnail('C:\\a.pdf', null, 0, 160);
    const second = await thumbnail('C:\\a.pdf', null, 0, 160);
    expect(first.url).toBe('data:image/png;base64,AAAA');
    expect(first.pageCount).toBe(3);
    expect(second).toBe(first);
    expect(render).toHaveBeenCalledTimes(1);
  });

  it('never runs more than three renders at once', async () => {
    let running = 0;
    let peak = 0;
    render.mockImplementation(async () => {
      running += 1;
      peak = Math.max(peak, running);
      await new Promise((r) => setTimeout(r, 5));
      running -= 1;
      return png;
    });
    await Promise.all(
      Array.from({ length: 10 }, (_, i) => thumbnail('C:\\a.pdf', null, i, 160)),
    );
    expect(peak).toBeLessThanOrEqual(3);
    expect(render).toHaveBeenCalledTimes(10);
  });

  it('does not cache failures, so unlocking a file can retry', async () => {
    render.mockRejectedValueOnce({ code: 'QW-PDF-ENCRYPTED', message: 'Locked.' });
    await expect(thumbnail('C:\\a.pdf', null, 0, 160)).rejects.toBeDefined();
    render.mockResolvedValue(png);
    await expect(thumbnail('C:\\a.pdf', 'open', 0, 160)).resolves.toBeDefined();
    await expect(thumbnail('C:\\a.pdf', null, 0, 160)).resolves.toBeDefined();
    expect(render).toHaveBeenCalledTimes(3);
  });

  it('keys by password presence, never by the password itself', async () => {
    render.mockResolvedValue(png);
    await thumbnail('C:\\a.pdf', 'secret-one', 0, 160);
    await thumbnail('C:\\a.pdf', 'secret-two', 0, 160);
    expect(render).toHaveBeenCalledTimes(1);
  });
});
