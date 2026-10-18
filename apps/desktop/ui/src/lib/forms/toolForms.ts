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
  visibleWhen?: Record<string, { field: string; equals: unknown }>;
}

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
  'split-by-text': { order: ['region', 'starts_with'], custom: ['region'] },
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
  'insert-pages': {
    order: ['mode', 'pages', 'position', 'count', 'page_size'],
    limits: { count: { min: 1, max: 100 } },
  },
};

/** The form hints of a tool, empty for tools without any. */
export function formFor(toolId: string): ToolForm {
  return toolForms[toolId] ?? {};
}
