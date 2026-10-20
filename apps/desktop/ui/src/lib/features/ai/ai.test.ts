import { describe, expect, it } from 'vitest';
import type { AiModel, ComponentEntry } from '../../ipc/generated';
import {
  SERVER_COMPONENT,
  mergeTerms,
  modelInstalled,
  modelOfComponent,
  partsProblem,
  partsToSend,
  rowsFromParts,
  serverInstalled,
  sizeText,
} from './ai';

const model = (id: string): AiModel => ({
  id,
  name: id,
  license: 'Apache-2.0',
  license_url: 'https://example.org/license',
  license_summary: 'Free.',
  file: `${id}.gguf`,
  size_mb: 100,
  note: '',
});

const entry = (id: string): ComponentEntry => ({
  id,
  version: '1',
  arch: 'x64',
  url: 'https://example.org/x',
  size: 1,
  sha256: '0'.repeat(64),
});

describe('components of the local model', () => {
  it('knows which component is which model', () => {
    const models = [model('qwen2.5-1.5b-instruct')];
    expect(modelOfComponent('model-qwen2.5-1.5b-instruct', models)?.id).toBe('qwen2.5-1.5b-instruct');
    expect(modelOfComponent('model-unknown', models)).toBeUndefined();
    expect(modelOfComponent('libreoffice', models)).toBeUndefined();
  });

  it('reads what is installed', () => {
    const installed = [entry('model-a'), entry(SERVER_COMPONENT), entry('tesseract')];
    expect(modelInstalled('a', installed)).toBe(true);
    expect(modelInstalled('b', installed)).toBe(false);
    expect(serverInstalled(installed)).toBe(true);
    expect(serverInstalled([entry('model-a')])).toBe(false);
  });

  it('writes sizes the way people read them', () => {
    expect(sizeText(386)).toBe('386 MB');
    expect(sizeText(1120)).toBe('1.1 GB');
    expect(sizeText(4680)).toBe('4.7 GB');
  });
});

describe('parts of Smart Split', () => {
  const row = (start: number, end: number) => ({ start, end, title: `p${start}` });

  it('accepts parts in order inside the document, with gaps', () => {
    expect(partsProblem([row(1, 3), row(4, 10)], 10)).toBeNull();
    expect(partsProblem([row(1, 2), row(5, 6)], 10)).toBeNull();
    expect(partsProblem([], 10)).toBeNull();
  });

  it('names the problem', () => {
    expect(partsProblem([row(1, 11)], 10)).toBe('smartSplit.problem.range');
    expect(partsProblem([row(0, 2)], 10)).toBe('smartSplit.problem.range');
    expect(partsProblem([row(3, 2)], 10)).toBe('smartSplit.problem.range');
    expect(partsProblem([row(1.5, 2)], 10)).toBe('smartSplit.problem.range');
    expect(partsProblem([row(Number.NaN, 2)], 10)).toBe('smartSplit.problem.range');
    expect(partsProblem([row(1, 3), row(3, 5)], 10)).toBe('smartSplit.problem.order');
    expect(partsProblem([row(5, 6), row(1, 2)], 10)).toBe('smartSplit.problem.order');
  });

  it('turns parts into rows and back with one-line titles', () => {
    const rows = rowsFromParts([{ start: 1, end: 2, title: 'A' }]);
    expect(rows).toEqual([{ start: 1, end: 2, title: 'A' }]);
    expect(partsToSend([{ start: 1, end: 2, title: '  An  \n  invoice ' }])).toEqual([
      { start: 1, end: 2, title: 'An invoice' },
    ]);
  });
});

describe('words found by the model', () => {
  it('are added after the given ones, once each', () => {
    expect(mergeTerms(['Acme', ' '], ['acme', 'Maria Gonzales', ' maria gonzales ', ''])).toEqual([
      'Acme',
      'Maria Gonzales',
    ]);
    expect(mergeTerms([], [])).toEqual([]);
  });
});
