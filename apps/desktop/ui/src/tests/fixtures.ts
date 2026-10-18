import type { DocumentReport, DropAnalysis } from '../lib/ipc/generated';

/** A report for a two-page, unencrypted, text PDF. */
export function report(overrides: Partial<DocumentReport> = {}): DocumentReport {
  return {
    file_size: 2048,
    locked: false,
    pdf_version: '1.7',
    page_count: 2,
    pages: [
      { number: 1, width_pt: 612, height_pt: 792, rotation: 0 },
      { number: 2, width_pt: 612, height_pt: 792, rotation: 0 },
    ],
    fonts: [{ name: 'Helvetica', embedded: false }],
    images: { count: 0, min_dpi: null, max_dpi: null, scanned_pages: 2 },
    encryption: null,
    permissions: null,
    signature_count: 0,
    has_forms: false,
    has_javascript: false,
    pdfa_claim: null,
    has_text_layer: true,
    embedded_files: 0,
    size_breakdown: { images: 0, fonts: 0, content: 100, other: 1948 },
    suggestions: [],
    ...overrides,
  };
}

/** A drop analysis where every path is a usable PDF. */
export function analysis(paths: string[]): DropAnalysis {
  return {
    files: paths.map((path) => ({
      path,
      name: path.split('\\').pop() ?? path,
      kind: 'pdf' as const,
      error: null,
    })),
    suggestions: paths.length > 1 ? ['merge'] : [],
  };
}
