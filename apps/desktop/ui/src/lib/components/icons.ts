import filePdf from '@phosphor-icons/core/assets/regular/file-pdf.svg?raw';
import folderOpen from '@phosphor-icons/core/assets/regular/folder-open.svg?raw';
import fileArrowUp from '@phosphor-icons/core/assets/regular/file-arrow-up.svg?raw';
import gear from '@phosphor-icons/core/assets/regular/gear.svg?raw';
import info from '@phosphor-icons/core/assets/regular/info.svg?raw';
import magnifyingGlass from '@phosphor-icons/core/assets/regular/magnifying-glass.svg?raw';

/** Phosphor Regular icons used by the app, imported one by one so unused icons are tree-shaken. */
export const icons = {
  'file-pdf': filePdf,
  'folder-open': folderOpen,
  'file-arrow-up': fileArrowUp,
  gear,
  info,
  'magnifying-glass': magnifyingGlass,
} as const;

/** Name of a bundled icon. */
export type IconName = keyof typeof icons;
