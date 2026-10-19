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


function choices(values: string[]) {
  return { oneOf: values.map((v) => ({ type: 'string', const: v, description: v })) };
}
const nullableText = { type: ['string', 'null'], default: null };

/** Protect: secrets, a cipher choice and a group of booleans. */
export const protectSchema = {
  type: 'object',
  properties: {
    cipher: { $ref: '#/$defs/Cipher', default: 'aes256' },
    owner_password: { anyOf: [{ $ref: '#/$defs/Secret' }, { type: 'null' }], default: null },
    permissions: {
      $ref: '#/$defs/Permissions',
      default: { annotate: true, assemble: true, copy: true, fill_forms: true, modify: true, print: true },
    },
    user_password: { anyOf: [{ $ref: '#/$defs/Secret' }, { type: 'null' }], default: null },
  },
  $defs: {
    Cipher: choices(['aes256', 'aes128']),
    Secret: { type: 'string' },
    Permissions: {
      type: 'object',
      properties: Object.fromEntries(
        ['annotate', 'assemble', 'copy', 'fill_forms', 'modify', 'print'].map((k) => [
          k,
          { type: 'boolean', default: true },
        ]),
      ),
    },
  },
};

/** Edit metadata: nullable texts that a switch can clear, and a nullable choice. */
export const editMetadataSchema = {
  type: 'object',
  properties: {
    author: nullableText,
    remove_all: { type: 'boolean', default: false },
    title: nullableText,
    trapped: { anyOf: [{ $ref: '#/$defs/Trapped' }, { type: 'null' }], default: null },
  },
  $defs: { Trapped: choices(['true', 'false', 'unknown']) },
};

/** Remove annotations: a list of choices that can be ticked together. */
export const removeAnnotationsSchema = {
  type: 'object',
  properties: {
    kinds: { type: 'array', default: ['all'], items: { $ref: '#/$defs/AnnotationKind' } },
    pages: { $ref: '#/$defs/PageRange', default: 'all' },
  },
  $defs: { AnnotationKind: choices(['all', 'highlights', 'links']), PageRange: pageRange },
};

/** Flatten: a resolution that only matters in one mode. */
export const flattenSchema = {
  type: 'object',
  properties: {
    dpi: { $ref: '#/$defs/FlattenDpi', default: '150' },
    mode: { $ref: '#/$defs/FlattenMode', default: 'forms-and-annotations' },
  },
  $defs: {
    FlattenDpi: choices(['75', '100', '150', '220']),
    FlattenMode: choices(['forms-and-annotations', 'everything']),
  },
};

/** Watermark: a kind that switches between text and picture fields, a color and a picture picker. */
export const watermarkSchema = {
  type: 'object',
  properties: {
    color: { type: 'string', default: '#808080' },
    image_path: nullableText,
    kind: { $ref: '#/$defs/WatermarkKind', default: 'text' },
    pages: { $ref: '#/$defs/PageRange', default: 'all' },
    text: { type: 'string', default: '' },
  },
  $defs: { WatermarkKind: choices(['text', 'image']), PageRange: pageRange },
};

/** Bookmarks: a switch, and rows with a level, a page and a title. */
export const bookmarksSchema = {
  type: 'object',
  properties: {
    auto_generate: { type: 'boolean', default: false },
    bookmarks: { type: 'array', default: [], items: { $ref: '#/$defs/BookmarkEntry' } },
  },
  $defs: {
    BookmarkEntry: {
      type: 'object',
      properties: {
        level: { type: 'integer', minimum: 1 },
        page: { type: 'integer', minimum: 1 },
        title: { type: 'string' },
      },
    },
  },
};

/** Attachments: an action, and lists of text lines for the paths and names. */
export const attachmentsSchema = {
  type: 'object',
  properties: {
    action: { $ref: '#/$defs/AttachmentAction', default: 'list' },
    files: { type: 'array', default: [], items: { type: 'string' } },
    names: { type: 'array', default: [], items: { type: 'string' } },
  },
  $defs: { AttachmentAction: choices(['list', 'add', 'extract', 'delete']) },
};

/** Crop: a method, whose region is drawn on the page only for one value. */
export const cropSchema = {
  type: 'object',
  properties: {
    method: { $ref: '#/$defs/CropMethod', default: 'margins' },
    region: byTextSchema.properties.region,
  },
  $defs: { CropMethod: choices(['margins', 'region', 'auto']), ...byTextSchema.$defs },
};

/** PDF to images: a format that decides which other fields matter. */
export const pdfToImagesSchema = {
  type: 'object',
  properties: {
    dpi: { type: 'integer', default: 150 },
    format: { $ref: '#/$defs/PictureFormat', default: 'jpg' },
    jpeg_quality: { type: 'integer', default: 90 },
    pages: { $ref: '#/$defs/PageRange', default: 'all' },
    single_tiff: { type: 'boolean', default: false },
  },
  $defs: { PictureFormat: choices(['jpg', 'png', 'tiff']), PageRange: pageRange },
};

/** HTML to PDF: a web address that needs no file, and a custom paper size. */
export const htmlSchema = {
  type: 'object',
  properties: {
    allow_network: { type: 'boolean', default: false },
    custom_width_mm: { type: 'number', default: 210 },
    custom_height_mm: { type: 'number', default: 297 },
    page_size: { $ref: '#/$defs/PaperSize', default: 'a4' },
    url: nullableText,
  },
  $defs: { PaperSize: choices(['a4', 'letter', 'custom']) },
};

/** OCR: a list of language codes, a choice and switches. */
export const ocrSchema = {
  type: 'object',
  properties: {
    deskew: { type: 'boolean', default: false },
    languages: { type: 'array', default: ['eng'], items: { type: 'string' } },
    output: { $ref: '#/$defs/OcrOutput', default: 'pdf' },
    pages: { $ref: '#/$defs/PageRange', default: 'all' },
    skip_pages_with_text: { type: 'boolean', default: true },
  },
  $defs: { OcrOutput: choices(['pdf', 'text']), PageRange: pageRange },
};

/** Deskew: a mode that decides between measuring and typed angles. */
export const deskewSchema = {
  type: 'object',
  properties: {
    angles: { type: 'array', default: [], items: { $ref: '#/$defs/PageAngle' } },
    max_angle_deg: { type: 'number', default: 15 },
    mode: { $ref: '#/$defs/DeskewMode', default: 'auto' },
    pages: { $ref: '#/$defs/PageRange', default: 'all' },
  },
  $defs: {
    DeskewMode: choices(['auto', 'manual']),
    PageRange: pageRange,
    PageAngle: {
      type: 'object',
      properties: { degrees: { type: 'number' }, page: { type: 'integer' } },
    },
  },
};

/** Scan to PDF: languages matter only when OCR is on. */
export const scanSchema = {
  type: 'object',
  properties: {
    auto_crop: { type: 'boolean', default: true },
    languages: { type: 'array', default: ['eng'], items: { type: 'string' } },
    ocr: { type: 'boolean', default: false },
  },
};

/** PDF to PowerPoint: a mode that decides between a picture resolution and OCR. */
export const slidesSchema = {
  type: 'object',
  properties: {
    dpi: { type: 'integer', default: 150 },
    languages: { type: 'array', default: ['eng'], items: { type: 'string' } },
    mode: { $ref: '#/$defs/SlideMode', default: 'editable-text' },
    ocr: { $ref: '#/$defs/ExportOcr', default: 'auto' },
    pages: { $ref: '#/$defs/PageRange', default: 'all' },
  },
  $defs: {
    SlideMode: choices(['editable-text', 'image-slides']),
    ExportOcr: choices(['auto', 'off', 'always']),
    PageRange: pageRange,
  },
};

/** PDF to Excel: a format, a grouping and a detection choice, and OCR languages. */
export const excelSchema = {
  type: 'object',
  properties: {
    detection: { $ref: '#/$defs/Detection', default: 'auto' },
    format: { $ref: '#/$defs/SheetFormat', default: 'xlsx' },
    grouping: { $ref: '#/$defs/SheetGrouping', default: 'per-table' },
    languages: { type: 'array', default: ['eng'], items: { type: 'string' } },
    ocr: { $ref: '#/$defs/ExportOcr', default: 'auto' },
    pages: { $ref: '#/$defs/PageRange', default: 'all' },
  },
  $defs: {
    Detection: choices(['auto', 'lattice', 'stream']),
    SheetFormat: choices(['xlsx', 'csv']),
    SheetGrouping: choices(['per-table', 'per-page']),
    ExportOcr: choices(['auto', 'off', 'always']),
    PageRange: pageRange,
  },
};
