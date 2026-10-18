import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';

/** Callbacks for window-level drag and drop. */
export interface DropHandlers {
  onhover: (hovering: boolean) => void;
  ondrop: (paths: string[]) => void;
}

/**
 * Listens for files dragged onto the window. Tauri reports real file paths, so the UI never
 * reads file contents itself. Works on every screen because it is attached to the webview.
 */
export async function listenForDrops(handlers: DropHandlers): Promise<UnlistenFn> {
  return getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === 'enter' || payload.type === 'over') handlers.onhover(true);
    else if (payload.type === 'leave') handlers.onhover(false);
    else if (payload.type === 'drop') {
      handlers.onhover(false);
      handlers.ondrop(payload.paths);
    }
  });
}
