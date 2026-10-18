import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import Inspect from '../lib/features/inspect/View.svelte';
import { report } from './fixtures';

describe('Inspect view', () => {
  it('shows the facts of an unencrypted PDF', () => {
    render(Inspect, { report: report() });
    expect(screen.getByText('1.7')).toBeTruthy();
    expect(screen.getByText('2.0 KB')).toBeTruthy();
    expect(screen.getByText(/2 pages at 612 by 792 pt/)).toBeTruthy();
    expect(screen.getByText('Helvetica')).toBeTruthy();
    expect(screen.getByText(/not embedded/)).toBeTruthy();
  });

  it('explains a locked file and hides contents it cannot know', () => {
    render(Inspect, {
      report: report({
        locked: true,
        pdf_version: null,
        page_count: 0,
        pages: [],
        encryption: { method: 'AES-256', key_bits: 256, revision: 6 },
        permissions: {
          print: true,
          copy: false,
          modify: false,
          annotate: true,
          fill_forms: true,
          assemble: false,
        },
      }),
    });
    expect(screen.getByText(/password protected/)).toBeTruthy();
    expect(screen.getByText('AES-256')).toBeTruthy();
    expect(screen.getByText('Copy: No')).toBeTruthy();
    expect(screen.queryByText('Fonts')).toBeNull();
  });

  it('flags JavaScript without running it and notes sampled image counts', () => {
    render(Inspect, {
      report: report({
        has_javascript: true,
        page_count: 800,
        images: { count: 3, min_dpi: 72, max_dpi: 300, scanned_pages: 500 },
      }),
    });
    expect(screen.getByText(/never runs it/)).toBeTruthy();
    expect(screen.getByText(/first 500 pages/)).toBeTruthy();
  });
});
