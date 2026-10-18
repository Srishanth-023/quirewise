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
  'insert-pages': {
    order: ['mode', 'pages', 'position', 'count', 'page_size'],
    limits: { count: { min: 1, max: 100 } },
  },
};

/** The form hints of a tool, empty for tools without any. */
export function formFor(toolId: string): ToolForm {
  return toolForms[toolId] ?? {};
}
