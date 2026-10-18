import { ipc, toAppError, type ToolDescriptor } from '../ipc/client';
import { tDynamic } from '../i18n';
import { toasts } from './toasts.svelte';

/** Display name for a tool: its i18n entry when there is one, otherwise its label. */
export function toolName(tool: ToolDescriptor): string {
  return tDynamic(`tool.${tool.id}`, tool.label);
}

/** The tool registry as the UI sees it. The UI never hardcodes a tool list. */
class Tools {
  items = $state<ToolDescriptor[]>([]);
  loaded = $state(false);
  current = $state<string | null>(null);

  async load(): Promise<void> {
    try {
      this.items = await ipc.listTools();
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      this.loaded = true;
    }
  }

  get selected(): ToolDescriptor | undefined {
    return this.items.find((t) => t.id === this.current);
  }

  select(id: string | null): void {
    this.current = id;
  }

  /** Tools whose name contains `query`, case-insensitive, grouped in registry order. */
  search(query: string): ToolDescriptor[] {
    const q = query.trim().toLowerCase();
    return q ? this.items.filter((t) => toolName(t).toLowerCase().includes(q)) : this.items;
  }

  /** Groups in first-seen order with their tools. */
  grouped(items: ToolDescriptor[] = this.items): { group: string; tools: ToolDescriptor[] }[] {
    const result: { group: string; tools: ToolDescriptor[] }[] = [];
    for (const tool of items) {
      const existing = result.find((g) => g.group === tool.group);
      if (existing) existing.tools.push(tool);
      else result.push({ group: tool.group, tools: [tool] });
    }
    return result;
  }
}

export const tools = new Tools();
