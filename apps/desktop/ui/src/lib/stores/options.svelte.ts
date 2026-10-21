import { untrack } from 'svelte';
import type { ToolDescriptor } from '../ipc/client';
import type { RunBatchRequest } from '../ipc/generated';
import {
  buildFields,
  defaultValues,
  itemDefaults,
  normalize,
  type Field,
  type JsonSchema,
  type Values,
} from '../forms/schema';
import { formFor } from '../forms/toolForms';

type Request = RunBatchRequest['options'];

/** A deeply reactive copy of `initial`. */
function reactive(initial: Values): Values {
  const state: Values = $state(initial);
  return state;
}

function allFields(tool: ToolDescriptor): Field[] {
  const schema = tool.options_schema as JsonSchema;
  return buildFields(schema, schema, formFor(tool.id).order);
}

/** The fields of a tool's form, minus the ones a dedicated editor owns. */
export function formFields(tool: ToolDescriptor): Field[] {
  const skip = formFor(tool.id).custom ?? [];
  return allFields(tool).filter((f) => !skip.includes(f.key));
}

/**
 * What the user chose for each tool, kept while they switch tools so a choice is not lost.
 *
 * Each tool's values are one reactive object that is created once and then changed in place, so
 * reading it from a derived value never writes state. Settings of per-file options belong to the
 * file, not to its position, so reordering the file list moves them along.
 */
export class ToolOptionsStore {
  // Plain maps on purpose: each value inside is its own reactive object, the maps only index them.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #values = new Map<string, Values>();
  /** Counts every edit, for readers that decide something from the values of the selected tool. */
  edits = $state(0);
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #perFile = new Map<string, Values>();

  /** Current values of a tool, starting from the defaults. */
  values(tool: ToolDescriptor): Values {
    let values = this.#values.get(tool.id);
    if (!values) {
      // Created outside any reaction, so no component's derived value owns (and later drops) it.
      const fresh: Values = untrack(() => reactive(defaultValues(allFields(tool))));
      this.#values.set(tool.id, fresh);
      values = fresh;
    }
    return values;
  }

  set(tool: ToolDescriptor, key: string, value: unknown): void {
    this.values(tool)[key] = value;
    this.edits = untrack(() => this.edits) + 1;
  }

  /** Replaces the values of a tool with saved options, keeping defaults for anything missing. */
  load(tool: ToolDescriptor, options: Record<string, unknown>): void {
    const rest = { ...options };
    delete rest.tool;
    const values = this.values(tool);
    Object.assign(values, defaultValues(allFields(tool)), JSON.parse(JSON.stringify(rest)));
    this.edits = untrack(() => this.edits) + 1;
  }

  /** Back to the defaults, including per-file settings. */
  reset(tool: ToolDescriptor): void {
    Object.assign(this.values(tool), defaultValues(allFields(tool)));
    this.edits = untrack(() => this.edits) + 1;
    const prefix = `${tool.id}.`;
    for (const [key, own] of this.#perFile) {
      if (!key.startsWith(prefix)) continue;
      const field = allFields(tool).find((f) => key.startsWith(`${prefix}${f.key}.`));
      if (field) Object.assign(own, itemDefaults(field));
    }
  }

  /** Settings of one file for a per-file field, created from the item defaults on first use. */
  perFile(tool: ToolDescriptor, field: Field, fileId: number): Values {
    const key = `${tool.id}.${field.key}.${fileId}`;
    let own = this.#perFile.get(key);
    if (!own) {
      const fresh: Values = untrack(() => reactive(itemDefaults(field)));
      this.#perFile.set(key, fresh);
      own = fresh;
    }
    return own;
  }

  /** The options to send: tool tag, every field normalized, per-file lists in file order. */
  request(tool: ToolDescriptor, fileIds: number[]): Request {
    const fields = allFields(tool);
    const values = { ...this.values(tool) };
    for (const key of formFor(tool.id).perFile ?? []) {
      const field = fields.find((f) => f.key === key);
      if (field) values[key] = fileIds.map((id) => ({ ...this.perFile(tool, field, id) }));
    }
    return { tool: tool.id, ...normalize(values, fields) } as Request;
  }
}

export const toolOptions = new ToolOptionsStore();
