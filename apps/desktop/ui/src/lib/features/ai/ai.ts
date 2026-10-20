// Pure helpers of the tools that use the local language model.

import type { AiModel, ComponentEntry, SplitPart } from '../../ipc/generated';

/** Id of the component that holds the llama.cpp server program. */
export const SERVER_COMPONENT = 'llama-cpp';

/** Id of the component that holds the file of a model of the catalog. */
export function modelComponent(id: string): string {
  return `model-${id}`;
}

/** The model of the catalog that a component id stands for, if it is one. */
export function modelOfComponent(componentId: string, models: AiModel[]): AiModel | undefined {
  if (!componentId.startsWith('model-')) return undefined;
  const id = componentId.slice('model-'.length);
  return models.find((m) => m.id === id);
}

/** True when the component of a model is installed. */
export function modelInstalled(id: string, installed: ComponentEntry[]): boolean {
  return installed.some((c) => c.id === modelComponent(id));
}

/** True when the program that runs the models is installed. */
export function serverInstalled(installed: ComponentEntry[]): boolean {
  return installed.some((c) => c.id === SERVER_COMPONENT);
}

/** Size of a download in megabytes, as people read it: 1.1 GB for the big ones. */
export function sizeText(mb: number): string {
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`;
}

/** A part of a document as the preview edits it: pages as typed, so a half-typed number is kept. */
export interface PartRow {
  start: number;
  end: number;
  title: string;
}

/** What is wrong with the parts, as an i18n key, or null when they can be made. */
export function partsProblem(parts: PartRow[], pageCount: number): string | null {
  let next = 1;
  for (const part of parts) {
    const whole = Number.isInteger(part.start) && Number.isInteger(part.end);
    if (!whole || part.start < 1 || part.end < part.start || part.end > pageCount) return 'smartSplit.problem.range';
    if (part.start < next) return 'smartSplit.problem.order';
    next = part.end + 1;
  }
  return null;
}

/** Parts the preview shows, from what the core proposed. */
export function rowsFromParts(parts: SplitPart[]): PartRow[] {
  return parts.map((p) => ({ start: p.start, end: p.end, title: p.title }));
}

/** Parts to send: titles trimmed to one line. */
export function partsToSend(rows: PartRow[]): SplitPart[] {
  return rows.map((r) => ({ start: r.start, end: r.end, title: r.title.replace(/\s+/g, ' ').trim() }));
}

/** The words the model found, added to the words already given, once each and without regard to case. */
export function mergeTerms(current: string[], found: string[]): string[] {
  const kept = current.filter((t) => t.trim() !== '');
  const seen = new Set(kept.map((t) => t.trim().toLowerCase()));
  const out = [...kept];
  for (const word of found) {
    const key = word.trim().toLowerCase();
    if (key === '' || seen.has(key)) continue;
    seen.add(key);
    out.push(word.trim());
  }
  return out;
}
