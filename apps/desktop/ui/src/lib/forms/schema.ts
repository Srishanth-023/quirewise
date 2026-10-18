// Turns a tool's options JSON Schema (generated from the Rust option structs) into a flat field
// model the form renders. Nothing here knows a tool by name: new tools get a form for free.

/** The subset of JSON Schema (2020-12) that schemars produces for option structs. */
export interface JsonSchema {
  $ref?: string;
  $defs?: Record<string, JsonSchema>;
  type?: string | string[];
  description?: string;
  default?: unknown;
  properties?: Record<string, JsonSchema>;
  items?: JsonSchema;
  oneOf?: JsonSchema[];
  anyOf?: JsonSchema[];
  const?: unknown;
  minimum?: number;
  maximum?: number;
}

export type FieldKind =
  | 'boolean'
  | 'integer'
  | 'number'
  | 'text'
  | 'choice'
  | 'multichoice'
  | 'group'
  | 'list'
  | 'unsupported';

export interface Choice {
  value: string;
  description: string;
}

export interface Field {
  key: string;
  kind: FieldKind;
  /** Doc comment from Rust, used when the UI has no translated label. */
  description: string;
  default: unknown;
  /** The value may be null, which is how "none" is stored. */
  nullable: boolean;
  choices?: Choice[];
  min?: number;
  max?: number;
  /** Fields of a `group`, or of each item of a `list`. */
  children?: Field[];
}

/** Values of a form, keyed by field. */
export type Values = Record<string, unknown>;

function resolve(schema: JsonSchema, root: JsonSchema): JsonSchema {
  if (!schema.$ref) return schema;
  const name = schema.$ref.replace('#/$defs/', '');
  const target = root.$defs?.[name];
  if (!target) return schema;
  // A property's own description and default win over those of the shared definition.
  return { ...target, ...(schema.description ? { description: schema.description } : {}), ...(schema.default !== undefined ? { default: schema.default } : {}) };
}

/** Splits `anyOf: [X, null]` and `type: [T, "null"]` into the real schema and a nullable flag. */
function unwrapNullable(schema: JsonSchema): { inner: JsonSchema; nullable: boolean } {
  if (schema.anyOf) {
    const rest = schema.anyOf.filter((s) => !(s.type === 'null'));
    if (rest.length === 1 && rest[0] && rest.length !== schema.anyOf.length) {
      const outer = { ...schema };
      delete outer.anyOf;
      return { inner: { ...outer, ...rest[0], description: schema.description ?? rest[0].description }, nullable: true };
    }
  }
  if (Array.isArray(schema.type)) {
    const rest = schema.type.filter((t) => t !== 'null');
    if (rest.length === 1 && rest[0]) {
      return { inner: { ...schema, type: rest[0] }, nullable: true };
    }
  }
  return { inner: schema, nullable: false };
}

function buildField(key: string, raw: JsonSchema, root: JsonSchema): Field {
  const { inner: first, nullable } = unwrapNullable(resolve(raw, root));
  const schema = resolve(first, root);
  const base = {
    key,
    description: schema.description ?? '',
    default: schema.default ?? (nullable ? null : undefined),
    nullable,
  };

  if (schema.oneOf && schema.oneOf.every((s) => typeof s.const === 'string')) {
    return {
      ...base,
      kind: 'choice',
      choices: schema.oneOf.map((s) => ({ value: String(s.const), description: s.description ?? '' })),
    };
  }
  switch (schema.type) {
    case 'boolean':
      return { ...base, kind: 'boolean', default: base.default ?? false };
    case 'integer':
      return { ...base, kind: 'integer', min: schema.minimum, max: schema.maximum };
    case 'number':
      return { ...base, kind: 'number', min: schema.minimum, max: schema.maximum };
    case 'string':
      return { ...base, kind: 'text', default: base.default === undefined ? '' : base.default };
    case 'object':
      return { ...base, kind: 'group', children: buildFields(schema, root) };
    case 'array': {
      const item = schema.items ? resolve(schema.items, root) : undefined;
      if (item?.oneOf && item.oneOf.every((s) => typeof s.const === 'string')) {
        return {
          ...base,
          kind: 'multichoice',
          default: base.default ?? [],
          choices: item.oneOf.map((s) => ({ value: String(s.const), description: s.description ?? '' })),
        };
      }
      if (item?.type === 'object') {
        return { ...base, kind: 'list', default: base.default ?? [], children: buildFields(item, root) };
      }
      return { ...base, kind: 'unsupported' };
    }
    default:
      return { ...base, kind: 'unsupported' };
  }
}

/**
 * Fields of an object schema. `order` lists keys to show first, in that order (JSON Schema sorts
 * properties alphabetically, which is rarely the order a person expects); the rest follow.
 */
export function buildFields(schema: JsonSchema, root: JsonSchema = schema, order: string[] = []): Field[] {
  const properties = schema.properties ?? {};
  const keys = Object.keys(properties);
  const rank = (k: string): number => {
    const i = order.indexOf(k);
    return i < 0 ? order.length : i;
  };
  keys.sort((a, b) => rank(a) - rank(b));
  return keys.map((key) => buildField(key, properties[key] as JsonSchema, root));
}

/** A deep copy of a JSON value. Schemas can arrive as reactive proxies, which `structuredClone` rejects. */
function clone<T>(value: T): T {
  return value === undefined ? value : (JSON.parse(JSON.stringify(value)) as T);
}

/** The default value of every field. */
export function defaultValues(fields: Field[]): Values {
  const values: Values = {};
  for (const field of fields) {
    if (field.kind === 'group' && !field.nullable) values[field.key] = defaultValues(field.children ?? []);
    else values[field.key] = clone(field.default);
  }
  return values;
}

/** Defaults of one list item, for example the per-file settings of a file. */
export function itemDefaults(field: Field): Values {
  return defaultValues(field.children ?? []);
}

/**
 * Values ready to send to the core: blank text of a nullable field becomes null, so "no filter"
 * is not an empty-string filter, and unset numbers fall back to the default.
 */
export function normalize(values: Values, fields: Field[]): Values {
  const out: Values = {};
  for (const field of fields) {
    const value = values[field.key];
    switch (field.kind) {
      case 'text':
        out[field.key] = field.nullable && (value === '' || value === undefined) ? null : value;
        break;
      case 'integer':
      case 'number':
        out[field.key] = typeof value === 'number' && Number.isFinite(value) ? value : field.default;
        break;
      case 'group':
        out[field.key] =
          value == null && field.nullable ? null : normalize((value ?? {}) as Values, field.children ?? []);
        break;
      case 'list':
        out[field.key] = ((value ?? []) as Values[]).map((item) => normalize(item, field.children ?? []));
        break;
      case 'unsupported':
        // Shapes the form cannot draw (such as the organizer's page list) are owned by a dedicated
        // editor, so they are sent exactly as that editor stored them.
        if (value !== undefined) out[field.key] = value;
        break;
      default:
        out[field.key] = value;
    }
  }
  return out;
}
