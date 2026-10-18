import { describe, expect, it } from 'vitest';
import { interpretKey, type KeyLike, type ShortcutContext } from './shortcuts';

const idle: ShortcutContext = {
  dialogOpen: false,
  jobRunning: false,
  canRun: true,
  focusIsInteractive: false,
};
const key = (k: string, extra: Partial<KeyLike> = {}): KeyLike => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...extra,
});

describe('interpretKey', () => {
  it('maps the documented shortcuts', () => {
    expect(interpretKey(key('k', { ctrlKey: true }), idle)).toBe('palette');
    expect(interpretKey(key('K', { ctrlKey: true }), idle)).toBe('palette');
    expect(interpretKey(key('o', { ctrlKey: true }), idle)).toBe('open-files');
    expect(interpretKey(key('F1'), idle)).toBe('shortcuts');
  });

  it('runs on Enter only when nothing else owns the key', () => {
    expect(interpretKey(key('Enter'), idle)).toBe('run');
    expect(interpretKey(key('Enter'), { ...idle, focusIsInteractive: true })).toBeNull();
    expect(interpretKey(key('Enter'), { ...idle, dialogOpen: true })).toBeNull();
    expect(interpretKey(key('Enter'), { ...idle, canRun: false })).toBeNull();
    expect(interpretKey(key('Enter'), { ...idle, jobRunning: true })).toBeNull();
  });

  it('asks to confirm cancelling only while a job runs and no dialog is open', () => {
    expect(interpretKey(key('Escape'), { ...idle, jobRunning: true })).toBe('confirm-cancel');
    expect(interpretKey(key('Escape'), { ...idle, jobRunning: true, dialogOpen: true })).toBeNull();
    expect(interpretKey(key('Escape'), idle)).toBeNull();
  });

  it('ignores unrelated keys', () => {
    expect(interpretKey(key('a'), idle)).toBeNull();
    expect(interpretKey(key('k'), idle)).toBeNull();
  });
});
