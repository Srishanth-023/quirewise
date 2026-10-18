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

const angle = {
  oneOf: ['0', '90', '180', '270'].map((v) => ({ type: 'string', const: v, description: v })),
};
const pageRange = { type: 'string' };

/** What `qw tools schema rotate` prints, trimmed to what the form reads. */
export const rotateSchema = {
  type: 'object',
  properties: {
    angle: { description: 'Rotation angle.', $ref: '#/$defs/RightAngle', default: '90' },
    pages: { description: 'Pages to rotate.', $ref: '#/$defs/PageRange', default: 'all' },
  },
  $defs: { PageRange: pageRange, RightAngle: angle },
};

/** Merge: booleans, a choice and per-file settings. */
export const mergeSchema = {
  type: 'object',
  properties: {
    bookmark_per_file: { type: 'boolean', default: true },
    forms: {
      $ref: '#/$defs/FormHandling',
      default: 'keep',
    },
    per_file: { type: 'array', default: [], items: { $ref: '#/$defs/MergeFileOptions' } },
  },
  $defs: {
    PageRange: pageRange,
    RightAngle: angle,
    FormHandling: {
      oneOf: ['keep', 'flatten'].map((v) => ({ type: 'string', const: v, description: v })),
    },
    MergeFileOptions: {
      type: 'object',
      properties: {
        pages: { $ref: '#/$defs/PageRange', default: 'all' },
        rotation: { $ref: '#/$defs/RightAngle', default: '0' },
      },
    },
  },
};

/** Split every N pages: one bounded number. */
export const everyNSchema = {
  type: 'object',
  properties: { n: { type: 'integer', format: 'uint32', minimum: 0, default: 1 } },
};

/** Organize: the page list is edited by a dedicated editor, so the form cannot draw it. */
export const organizeSchema = {
  type: 'object',
  properties: {
    pages: { type: 'array', default: [], items: { oneOf: [{ type: 'object', properties: {} }] } },
  },
};
export const byTextSchema = {
  type: 'object',
  properties: {
    region: { anyOf: [{ $ref: '#/$defs/Region' }, { type: 'null' }], default: null },
    starts_with: { type: ['string', 'null'], default: null },
  },
  $defs: {
    Region: {
      type: 'object',
      properties: Object.fromEntries(
        ['x', 'y', 'width', 'height'].map((k) => [k, { type: 'number', format: 'float' }]),
      ),
    },
  },
};

