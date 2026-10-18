import arrowClockwise from '@phosphor-icons/core/assets/regular/arrow-clockwise.svg?raw';
import arrowDown from '@phosphor-icons/core/assets/regular/arrow-down.svg?raw';
import arrowUp from '@phosphor-icons/core/assets/regular/arrow-up.svg?raw';
import command from '@phosphor-icons/core/assets/regular/command.svg?raw';
import fileArrowUp from '@phosphor-icons/core/assets/regular/file-arrow-up.svg?raw';
import filePdf from '@phosphor-icons/core/assets/regular/file-pdf.svg?raw';
import folderOpen from '@phosphor-icons/core/assets/regular/folder-open.svg?raw';
import gear from '@phosphor-icons/core/assets/regular/gear.svg?raw';
import house from '@phosphor-icons/core/assets/regular/house.svg?raw';
import image from '@phosphor-icons/core/assets/regular/image.svg?raw';
import info from '@phosphor-icons/core/assets/regular/info.svg?raw';
import keyboard from '@phosphor-icons/core/assets/regular/keyboard.svg?raw';
import lock from '@phosphor-icons/core/assets/regular/lock.svg?raw';
import magnifyingGlass from '@phosphor-icons/core/assets/regular/magnifying-glass.svg?raw';
import play from '@phosphor-icons/core/assets/regular/play.svg?raw';
import stop from '@phosphor-icons/core/assets/regular/stop.svg?raw';
import trash from '@phosphor-icons/core/assets/regular/trash.svg?raw';
import warning from '@phosphor-icons/core/assets/regular/warning.svg?raw';
import x from '@phosphor-icons/core/assets/regular/x.svg?raw';

/** Phosphor Regular icons used by the app, imported one by one so unused icons are tree-shaken. */
export const icons = {
  'arrow-clockwise': arrowClockwise,
  'arrow-down': arrowDown,
  'arrow-up': arrowUp,
  command,
  'file-arrow-up': fileArrowUp,
  'file-pdf': filePdf,
  'folder-open': folderOpen,
  gear,
  house,
  image,
  info,
  keyboard,
  lock,
  'magnifying-glass': magnifyingGlass,
  play,
  stop,
  trash,
  warning,
  x,
} as const;

/** Name of a bundled icon. */
export type IconName = keyof typeof icons;
