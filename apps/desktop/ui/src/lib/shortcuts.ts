/** What a key press asks the app to do. */
export type ShortcutAction = 'palette' | 'open-files' | 'run' | 'confirm-cancel' | 'shortcuts';

/** The facts needed to decide what a key means right now. */
export interface ShortcutContext {
  /** A modal dialog is open; it handles Escape itself. */
  dialogOpen: boolean;
  jobRunning: boolean;
  /** A tool is chosen and there are files to run it on. */
  canRun: boolean;
  /** Focus is in a text field, select, button or link, where Enter belongs to that control. */
  focusIsInteractive: boolean;
}

/** The subset of KeyboardEvent the shortcut logic reads. */
export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** Maps a key press to an action, or null when the app should ignore it. */
export function interpretKey(event: KeyLike, context: ShortcutContext): ShortcutAction | null {
  const mod = event.ctrlKey || event.metaKey;
  const key = event.key.toLowerCase();
  if (mod && !event.altKey && key === 'k') return 'palette';
  if (mod && !event.altKey && key === 'o') return 'open-files';
  if (event.key === 'F1') return 'shortcuts';
  if (event.key === 'Escape' && !context.dialogOpen && context.jobRunning) return 'confirm-cancel';
  if (
    event.key === 'Enter' &&
    !mod &&
    !context.dialogOpen &&
    !context.focusIsInteractive &&
    context.canRun &&
    !context.jobRunning
  ) {
    return 'run';
  }
  return null;
}

/** True when the element is a control that uses Enter or Space itself. */
export function isInteractive(element: Element | null): boolean {
  if (!element) return false;
  return element.closest('input, textarea, select, button, a, [role="option"], [contenteditable="true"]') !== null;
}
