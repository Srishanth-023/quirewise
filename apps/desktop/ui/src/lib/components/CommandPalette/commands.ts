/** One entry in the command palette. */
export interface Command {
  id: string;
  group: 'tools' | 'recipes' | 'files' | 'actions';
  label: string;
  /** Extra text shown after the label, for example a shortcut or folder. */
  hint?: string;
  run: () => void;
}

/**
 * Filters and ranks commands for a query: labels that start with the query come first, then
 * labels that contain it, then labels that contain all its characters in order.
 */
export function rankCommands(commands: Command[], query: string): Command[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  const scored: { command: Command; score: number }[] = [];
  for (const command of commands) {
    const label = command.label.toLowerCase();
    let score = 0;
    if (label.startsWith(q)) score = 3;
    else if (label.includes(q)) score = 2;
    else if (isSubsequence(q, label)) score = 1;
    if (score > 0) scored.push({ command, score });
  }
  // Array.sort is stable, so equal scores keep their original order.
  return scored.sort((a, b) => b.score - a.score).map((s) => s.command);
}

function isSubsequence(needle: string, haystack: string): boolean {
  let i = 0;
  for (const ch of haystack) {
    if (ch === needle[i]) i += 1;
    if (i === needle.length) return true;
  }
  return needle.length === 0;
}
