export interface LintFinding {
  line: number;
  rule: string;
  message: string;
}

/** Lints one text file. The path is used only for rule scoping. */
export function lintText(path: string, text: string): LintFinding[];
