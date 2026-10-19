// Presentation hints that JSON Schema cannot carry. Everything else comes from the schema.

export interface ToolForm {
  /** Fields shown first, in this order. Others follow alphabetically. */
  order?: string[];
  /** Fields a dedicated editor handles, so the generic form skips them. */
  custom?: string[];
  /** Bounds the form enforces before the core validates them again. */
  limits?: Record<string, { min?: number; max?: number }>;
  /** Fields that hold one entry per input file, in the order of the file list. */
  perFile?: string[];
  /** Text fields typed hidden, such as passwords. */
  secret?: string[];
  /** Text fields with a "remove" switch, where blank means "keep the current value". */
  clearable?: string[];
  /** Fields shown only while another field has a given value. */
  visibleWhen?: Record<string, ShowRule>;
  /** Text fields picked with a color control. */
  color?: string[];
  /** Text or line-list fields with a button that opens the file chooser. */
  picker?: Record<string, 'image' | 'files'>;
  /** Lists of objects edited as rows the person can add, remove and move. */
  rows?: string[];
  /** Draw the region picker over the first page; `when` limits it to one value of another field. */
  regionPicker?: { when?: ShowRule };
}

/** What the color control shows while the text is not a valid #RRGGBB value. */
export const FALLBACK_COLOR = '#000000';

/** Show something while a field has one value (`equals`) or any of several (`in`). */
export interface ShowRule {
  field: string;
  equals?: unknown;
  in?: unknown[];
}

/** True when the rule is met by the current values, or when there is no rule. */
export function ruleMet(rule: ShowRule | undefined, values: Record<string, unknown>): boolean {
  if (!rule) return true;
  const current = values[rule.field];
  return rule.in ? rule.in.includes(current) : current === rule.equals;
}

const only = (keys: string[], rule: ShowRule): Record<string, ShowRule> =>
  Object.fromEntries(keys.map((key) => [key, rule]));

export const toolForms: Record<string, ToolForm> = {
  merge: {
    order: ['per_file', 'bookmark_per_file', 'table_of_contents', 'normalize_page_size', 'forms'],
    perFile: ['per_file'],
  },
  'alternate-mix': { order: ['pages_per_turn', 'per_file'], limits: { pages_per_turn: { min: 1, max: 1000 } }, perFile: ['per_file'] },
  organize: { custom: ['pages'] },
  'split-ranges': { order: ['ranges', 'merge_into_one'] },
  'split-every-n': { limits: { n: { min: 1 } } },
  'extract-pages': { order: ['pages', 'one_file'] },
  'remove-pages': { order: ['pages'] },
  'split-by-bookmarks': { order: ['level', 'name_from_bookmark'], limits: { level: { min: 1 } } },
  'split-in-half': { order: ['direction', 'skip_first', 'skip_last', 'right_to_left'] },
  'split-by-size': { limits: { max_size_mb: { min: 1, max: 2048 } } },
  'split-by-text': { order: ['region', 'starts_with'], custom: ['region'], regionPicker: {} },
  rotate: { order: ['angle', 'pages'] },
  compress: {
    order: ['preset', 'jpeg_quality', 'max_image_ppi', 'grayscale', 'strip_metadata', 'object_streams'],
    limits: { jpeg_quality: { min: 1, max: 100 } },
  },
  repair: {},
  linearize: { order: ['enabled'] },
  'remove-annotations': { order: ['kinds', 'pages'] },
  flatten: {
    order: ['mode', 'dpi'],
    visibleWhen: { dpi: { field: 'mode', equals: 'everything' } },
  },
  'edit-metadata': {
    order: [
      'title',
      'author',
      'subject',
      'keywords',
      'creator',
      'producer',
      'creation_date',
      'modification_date',
      'trapped',
      'remove_all',
    ],
    clearable: ['title', 'author', 'subject', 'keywords', 'creator', 'producer', 'creation_date', 'modification_date'],
    visibleWhen: Object.fromEntries(
      [
        'title',
        'author',
        'subject',
        'keywords',
        'creator',
        'producer',
        'creation_date',
        'modification_date',
        'trapped',
      ].map((key) => [key, { field: 'remove_all', equals: false }]),
    ),
  },
  protect: {
    order: ['user_password', 'owner_password', 'cipher', 'permissions'],
    secret: ['user_password', 'owner_password'],
  },
  unlock: {},
  sanitize: {
    order: [
      'metadata',
      'xmp',
      'javascript',
      'embedded_files',
      'hidden_layers',
      'form_actions',
      'thumbnails',
      'private_data',
    ],
  },
  'page-numbers': {
    order: [
      'pages',
      'template',
      'style',
      'position',
      'offset_x_pt',
      'offset_y_pt',
      'start_number',
      'skip_first_page',
      'facing_pages',
      'font',
      'bold',
      'italic',
      'size_pt',
      'color',
    ],
    color: ['color'],
    limits: { size_pt: { min: 1, max: 400 } },
  },
  'header-footer': {
    order: [
      'header_left',
      'header_center',
      'header_right',
      'footer_left',
      'footer_center',
      'footer_right',
      'pages',
      'font',
      'bold',
      'italic',
      'size_pt',
      'color',
      'margin_pt',
    ],
    color: ['color'],
    limits: { size_pt: { min: 1, max: 400 }, margin_pt: { min: 0, max: 400 } },
  },
  bates: {
    order: [
      'prefix',
      'suffix',
      'digits',
      'start',
      'continue_across_files',
      'position',
      'offset_x_pt',
      'offset_y_pt',
      'font',
      'bold',
      'italic',
      'size_pt',
      'color',
    ],
    color: ['color'],
    limits: { digits: { min: 1, max: 12 }, size_pt: { min: 1, max: 400 } },
  },
  watermark: {
    order: [
      'kind',
      'text',
      'image_path',
      'image_width_percent',
      'pages',
      'position',
      'tile',
      'rotation_deg',
      'transparency',
      'layer',
      'font',
      'bold',
      'italic',
      'size_pt',
      'color',
      'offset_x_pt',
      'offset_y_pt',
    ],
    color: ['color'],
    picker: { image_path: 'image' },
    limits: {
      transparency: { min: 1, max: 100 },
      rotation_deg: { min: 0, max: 360 },
      image_width_percent: { min: 1, max: 100 },
      size_pt: { min: 1, max: 400 },
    },
    visibleWhen: {
      ...only(['text', 'font', 'bold', 'italic', 'size_pt', 'color'], { field: 'kind', equals: 'text' }),
      ...only(['image_path', 'image_width_percent'], { field: 'kind', equals: 'image' }),
    },
  },
  crop: {
    order: ['method', 'margins', 'unit', 'region', 'auto_padding_mm', 'pages', 'apply_to_media_box'],
    custom: ['region'],
    regionPicker: { when: { field: 'method', equals: 'region' } },
    visibleWhen: {
      ...only(['margins', 'unit'], { field: 'method', equals: 'margins' }),
      auto_padding_mm: { field: 'method', equals: 'auto' },
    },
  },
  resize: {
    order: ['size', 'custom_width_mm', 'custom_height_mm', 'margin_mm', 'scale_content', 'match_orientation'],
    visibleWhen: only(['custom_width_mm', 'custom_height_mm'], { field: 'size', equals: 'custom' }),
  },
  flip: { order: ['direction', 'pages'] },
  'n-up': { order: ['per_sheet', 'order', 'preserve_page_size', 'right_to_left'] },
  grayscale: {
    order: ['apply_to', 'raster_fallback', 'dpi'],
    visibleWhen: { dpi: { field: 'raster_fallback', equals: true } },
  },
  bookmarks: {
    order: ['auto_generate', 'auto_max_level', 'bookmarks'],
    rows: ['bookmarks'],
    limits: { auto_max_level: { min: 1, max: 6 } },
    visibleWhen: {
      auto_max_level: { field: 'auto_generate', equals: true },
      bookmarks: { field: 'auto_generate', equals: false },
    },
  },
  'rename-by-content': {
    order: ['pattern', 'page', 'region'],
    custom: ['region'],
    regionPicker: {},
    limits: { page: { min: 1 } },
  },
  attachments: {
    order: ['action', 'files', 'names'],
    picker: { files: 'files' },
    visibleWhen: {
      files: { field: 'action', equals: 'add' },
      names: { field: 'action', in: ['extract', 'delete'] },
    },
  },
  'page-labels': {
    order: ['remove_all', 'ranges'],
    rows: ['ranges'],
    visibleWhen: { ranges: { field: 'remove_all', equals: false } },
  },
  'insert-pages': {
    order: ['mode', 'pages', 'position', 'count', 'page_size'],
    limits: { count: { min: 1, max: 100 } },
  },
};

/** The form hints of a tool, empty for tools without any. */
export function formFor(toolId: string): ToolForm {
  return toolForms[toolId] ?? {};
}
