import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import ProgressBar from '../lib/components/ProgressBar/ProgressBar.svelte';
import DropZone from '../lib/components/DropZone/DropZone.svelte';
import { t } from '../lib/i18n';

describe('ProgressBar', () => {
  it('exposes determinate progress with page counts', () => {
    render(ProgressBar, { current: 14, total: 120, label: 'Compressing report.pdf' });
    const bar = screen.getByRole('progressbar', { name: 'Compressing report.pdf' });
    expect(bar.getAttribute('aria-valuenow')).toBe('14');
    expect(bar.getAttribute('aria-valuemax')).toBe('120');
    expect(screen.getByText('Page 14 of 120')).toBeTruthy();
  });
});

describe('DropZone', () => {
  it('offers a keyboard reachable file chooser', () => {
    render(DropZone, { onchoose: () => {} });
    expect(screen.getByRole('button', { name: t('drop.browse') })).toBeTruthy();
  });
});

describe('i18n', () => {
  it('formats ICU plurals without string concatenation', () => {
    expect(t('progress.files', { count: 1 })).toBe('1 file');
    expect(t('progress.files', { count: 3 })).toBe('3 files');
  });
});
