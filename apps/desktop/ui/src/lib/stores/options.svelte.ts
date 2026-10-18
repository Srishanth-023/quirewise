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
class ToolOptionsStore {
  #values = new Map<string, Values>();
  #perFile = new Map<string, Values>();

  /** Current values of a tool, starting from the defaults. */
  values(tool: ToolDescriptor): Values {
    let values = this.#values.get(tool.id);
    if (!values) {
      const fresh: Values = $state(defaultValues(allFields(tool)));
      this.#values.set(tool.id, fresh);
      values = fresh;
    }
    return values;
  }

  set(tool: ToolDescriptor, key: string, value: unknown): void {
    this.values(tool)[key] = value;
  }

  /** Back to the defaults, including per-file settings. */
  reset(tool: ToolDescriptor): void {
    Object.assign(this.values(tool), defaultValues(allFields(tool)));
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
      const fresh: Values = $state(itemDefaults(field));
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
