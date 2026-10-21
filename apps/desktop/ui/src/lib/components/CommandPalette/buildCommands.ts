import { t } from '../../i18n';
import type { RecentFile, Recipe, ToolDescriptor } from '../../ipc/client';
import { toolName } from '../../stores/tools.svelte';
import type { Command } from './commands';

/** What the palette can do, supplied by the app shell. */
export interface PaletteActions {
  selectTool: (id: string) => void;
  addFile: (path: string) => void;
  openFiles: () => void;
  showShortcuts: () => void;
  openSettings: () => void;
  useRecipe: (id: string) => void;
  openAbout: () => void;
  clearFiles: () => void;
}

/** Builds the palette entries: every registered tool, recent files, and app actions. */
export function buildCommands(
  tools: ToolDescriptor[],
  recent: RecentFile[],
  actions: PaletteActions,
  recipes: Recipe[] = [],
): Command[] {
  return [
    ...tools.map(
      (tool): Command => ({
        id: `tool-${tool.id}`,
        group: 'tools',
        label: toolName(tool),
        run: () => actions.selectTool(tool.id),
      }),
    ),
    ...recipes.map(
      (recipe): Command => ({
        id: `recipe-${recipe.id}`,
        group: 'recipes',
        label: recipe.name,
        run: () => actions.useRecipe(recipe.id ?? ''),
      }),
    ),
    ...recent.slice(0, 10).map(
      (file): Command => ({
        id: `recent-${file.path}`,
        group: 'files',
        label: file.name,
        hint: file.path,
        run: () => actions.addFile(file.path),
      }),
    ),
    { id: 'open', group: 'actions', label: t('action.open'), hint: 'Ctrl+O', run: actions.openFiles },
    { id: 'shortcuts', group: 'actions', label: t('action.shortcuts'), hint: 'F1', run: actions.showShortcuts },
    { id: 'settings', group: 'actions', label: t('action.settings'), run: actions.openSettings },
    { id: 'about', group: 'actions', label: t('action.about'), run: actions.openAbout },
    { id: 'clear', group: 'actions', label: t('action.clearFiles'), run: actions.clearFiles },
  ];
}
