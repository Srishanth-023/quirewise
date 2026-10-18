// Walks the repository and fails on banned visual patterns. Run via `pnpm lint`.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lintText } from './visualLintRules.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..');
const SKIP_DIRS = new Set([
  'node_modules', 'target', '.git', 'vendor', 'dist', 'gen', 'sbom', 'reports',
  'coverage', 'fonts', 'fixtures', 'golden', '.svelte-kit',
]);
const SKIP_FILES = new Set(['pnpm-lock.yaml', 'Cargo.lock', 'LICENSE']);
const TEXT_EXT = new Set([
  '.rs', '.toml', '.md', '.json', '.yml', '.yaml', '.ps1', '.ts', '.js', '.mjs', '.mts',
  '.svelte', '.css', '.html', '.txt', '.gitignore', '.gitattributes', '.editorconfig',
]);

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) yield* walk(full);
    else if (!SKIP_FILES.has(name)) yield full;
  }
}

let failures = 0;
for (const file of walk(root)) {
  const rel = relative(root, file).replaceAll('\\', '/');
  const dot = rel.lastIndexOf('.');
  const ext = dot < 0 ? '' : rel.slice(dot);
  const base = rel.split('/').pop() ?? '';
  if (!TEXT_EXT.has(ext) && !TEXT_EXT.has(base)) continue;
  // Rule definitions and their tests must mention the banned patterns they detect.
  if (rel.endsWith('visualLintRules.mjs') || rel.endsWith('visualLint.test.ts')) continue;
  for (const f of lintText(rel, readFileSync(file, 'utf8'))) {
    failures += 1;
    console.error(`${rel}:${f.line}  [${f.rule}] ${f.message}`);
  }
}

if (failures > 0) {
  console.error(`visual-lint: ${failures} finding(s)`);
  process.exit(1);
}
console.log('visual-lint: clean');
