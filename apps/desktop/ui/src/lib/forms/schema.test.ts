import { describe, expect, it } from 'vitest';
import { buildFields, defaultValues, itemDefaults, normalize, type Field, type JsonSchema } from './schema';

// Trimmed copies of what `qw tools schema <id>` prints.
const angle: JsonSchema = {
  oneOf: [
    { type: 'string', const: '0', description: 'No rotation.' },
    { type: 'string', const: '90', description: '90 degrees clockwise.' },
  ],
};

const rotate: JsonSchema = {
  type: 'object',
  properties: {
    angle: { description: 'Rotation angle.', $ref: '#/$defs/RightAngle', default: '90' },
    pages: { description: 'Pages to rotate.', $ref: '#/$defs/PageRange', default: 'all' },
  },
  $defs: { PageRange: { type: 'string' }, RightAngle: angle },
};

const merge: JsonSchema = {
  type: 'object',
  properties: {
    bookmark_per_file: { type: 'boolean', default: true },
    per_file: { type: 'array', default: [], items: { $ref: '#/$defs/MergeFileOptions' } },
  },
  $defs: {
    PageRange: { type: 'string' },
    RightAngle: angle,
    MergeFileOptions: {
      type: 'object',
      properties: {
        pages: { $ref: '#/$defs/PageRange', default: 'all' },
        rotation: { $ref: '#/$defs/RightAngle', default: '0' },
      },
    },
  },
};

const byText: JsonSchema = {
  type: 'object',
  properties: {
    region: { anyOf: [{ $ref: '#/$defs/Region' }, { type: 'null' }], default: null },
    starts_with: { type: ['string', 'null'], default: null },
  },
  $defs: {
    Region: {
      type: 'object',
      properties: { x: { type: 'number' }, y: { type: 'number' } },
    },
  },
};

describe('buildFields', () => {
  it('maps enums to choices and strings to text, keeping defaults and descriptions', () => {
    const [a, p] = buildFields(rotate);
    expect(a).toMatchObject({ key: 'angle', kind: 'choice', default: '90', description: 'Rotation angle.' });
    expect(a?.choices?.map((c) => c.value)).toEqual(['0', '90']);
    expect(p).toMatchObject({ key: 'pages', kind: 'text', default: 'all' });
  });

  it('orders the listed keys first and the rest after', () => {
    expect(buildFields(rotate, rotate, ['pages', 'angle']).map((f) => f.key)).toEqual(['pages', 'angle']);
    expect(buildFields(merge, merge, ['per_file']).map((f) => f.key)).toEqual(['per_file', 'bookmark_per_file']);
  });

  it('describes lists of objects through their item fields', () => {
    const list = buildFields(merge).find((f) => f.key === 'per_file');
    expect(list?.kind).toBe('list');
    expect(itemDefaults(list as Field)).toEqual({ pages: 'all', rotation: '0' });
  });

  it('treats anyOf with null and type arrays as nullable', () => {
    const [region, startsWith] = buildFields(byText);
    expect(region).toMatchObject({ key: 'region', kind: 'group', nullable: true, default: null });
    expect(startsWith).toMatchObject({ key: 'starts_with', kind: 'text', nullable: true, default: null });
  });

  it('keeps nested numbers so a region survives normalizing', () => {
    const [region] = buildFields(byText);
    const value = { x: 0.1, y: 0.2 };
    expect(normalize({ region: value, starts_with: null }, buildFields(byText)).region).toEqual(value);
    expect(region?.children?.map((c) => c.kind)).toEqual(['number', 'number']);
  });

  it('marks shapes it cannot render as unsupported and passes their values through', () => {
    const odd: JsonSchema = { type: 'object', properties: { x: { type: 'array', items: { type: 'string' } } } };
    expect(buildFields(odd)[0]?.kind).toBe('unsupported');
    expect(normalize({ x: ['a'] }, buildFields(odd))).toEqual({ x: ['a'] });
  });
});

describe('values', () => {
  it('starts from every default', () => {
    expect(defaultValues(buildFields(rotate))).toEqual({ angle: '90', pages: 'all' });
    expect(defaultValues(buildFields(merge))).toEqual({ bookmark_per_file: true, per_file: [] });
  });

  it('sends blank nullable text as null and unset numbers as the default', () => {
    const fields = buildFields(byText);
    expect(normalize({ region: null, starts_with: '' }, fields)).toEqual({ region: null, starts_with: null });
    expect(normalize({ region: null, starts_with: 'INV' }, fields).starts_with).toBe('INV');
    const n: JsonSchema = { type: 'object', properties: { n: { type: 'integer', default: 1 } } };
    expect(normalize({ n: Number.NaN }, buildFields(n))).toEqual({ n: 1 });
    expect(normalize({ n: 5 }, buildFields(n))).toEqual({ n: 5 });
  });

  it('normalizes the items of a list', () => {
    const fields = buildFields(merge);
    const out = normalize({ bookmark_per_file: false, per_file: [{ pages: '1-2', rotation: '90' }] }, fields);
    expect(out).toEqual({ bookmark_per_file: false, per_file: [{ pages: '1-2', rotation: '90' }] });
  });
});
