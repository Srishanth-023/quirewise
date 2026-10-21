// Chain rules for the recipe builder. The core checks them again when a recipe is saved or run;
// the builder uses them to offer only the tools that fit and to explain what is wrong.

export type Role = 'source' | 'middle' | 'last' | 'excluded';

/** Where each tool may sit, as the core reports it. */
export type Roles = Record<string, Role>;

/** The first reason a chain of tools cannot run, or null when it can. */
export function chainProblem(
  toolIds: string[],
  roles: Roles,
): { step: number; reason: 'excluded' | 'source-first' | 'last-only' } | null {
  const last = toolIds.length - 1;
  for (const [i, id] of toolIds.entries()) {
    const role = roles[id] ?? 'excluded';
    if (role === 'excluded') return { step: i + 1, reason: 'excluded' };
    if (role === 'source' && i !== 0) return { step: i + 1, reason: 'source-first' };
    if (role === 'last' && i !== last) return { step: i + 1, reason: 'last-only' };
  }
  return null;
}

/** True when `candidate` can be appended to a chain that already holds `toolIds`. */
export function canAppend(toolIds: string[], candidate: string, roles: Roles): boolean {
  return chainProblem([...toolIds, candidate], roles) === null;
}

/** Tools that can be added after the current steps, in the order given. */
export function addable<T extends { id: string }>(toolList: T[], toolIds: string[], roles: Roles): T[] {
  return toolList.filter((tool) => canAppend(toolIds, tool.id, roles));
}

/** True when the first step converts a file that is not a PDF, so a folder run also takes those. */
export function startsWithSource(toolIds: string[], roles: Roles): boolean {
  const first = toolIds[0];
  return first !== undefined && roles[first] === 'source';
}

/** The last path segment, for showing a file by name. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
