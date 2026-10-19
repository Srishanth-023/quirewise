// The colors the editor puts on a page. They end up inside the PDF, so they do not follow the
// theme of the window (the page is always paper) and are kept here rather than in the styles.

export const palette = {
  /** Text, and the default of every text color. */
  black: '#000000',
  /** The color of a patch and the paper under it. */
  white: '#FFFFFF',
  /** Lines and outlines of shapes, underline and strike-out. */
  mark: '#D32F2F',
  /** Highlights, and what a shape is filled with first. */
  highlight: '#FFEB3B',
  /** Sticky notes. */
  note: '#FFD54F',
  /** Freehand drawing. */
  pen: '#1565C0',
  /** Signatures, initials and dates. */
  ink: '#111827',
} as const;
