// Visual lint rules (MASTER_PROMPT.md section 13, AGENTS.md section 4).
// Pure functions so the rules themselves are unit tested.

const STYLE_EXT = new Set(['.css', '.svelte', '.html']);
const CODE_EXT = new Set(['.css', '.svelte', '.html', '.ts', '.js', '.mjs', '.json']);
const EM_DASH = '—';
const MAX_RADIUS_PX = 4;

/** @param {string} path */
function ext(path) {
  const i = path.lastIndexOf('.');
  return i < 0 ? '' : path.slice(i).toLowerCase();
}

/** @param {string} path */
function norm(path) {
  return path.replaceAll('\\', '/');
}

/**
 * @param {string} value a border-radius declaration value
 * @returns {boolean} true when any component exceeds the 4 px limit
 */
function radiusTooLarge(value) {
  for (const token of value.split(/[\s/]+/)) {
    const m = /^(-?\d*\.?\d+)(px|rem|em|%)?$/.exec(token);
    if (!m) continue;
    const n = Number(m[1]);
    if (n === 0) continue;
    const unit = m[2] ?? '';
    if (unit === 'px' && n > MAX_RADIUS_PX) return true;
    if ((unit === 'rem' || unit === 'em') && n * 16 > MAX_RADIUS_PX) return true;
    if (unit === '%') return true;
  }
  return false;
}

/**
 * Lints one text file.
 * @param {string} path repo-relative or absolute path, used for rule scoping only
 * @param {string} text file contents
 * @returns {{ line: number; rule: string; message: string }[]}
 */
export function lintText(path, text) {
  const p = norm(path);
  const e = ext(p);
  const findings = [];
  const isTokens = p.endsWith('/design/tokens.css') || p === 'tokens.css';
  const isStyle = STYLE_EXT.has(e);
  const isCode = CODE_EXT.has(e);
  const lines = text.split(/\r?\n/);

  lines.forEach((line, idx) => {
    const add = (rule, message) => findings.push({ line: idx + 1, rule, message });

    if (line.includes(EM_DASH)) add('em-dash', 'Em dash (U+2014) is banned. Use a comma, colon, parentheses or hyphen.');

    if (isCode && /\p{Emoji_Presentation}|️/u.test(line)) add('emoji', 'Emoji code points are banned.');

    if (isStyle) {
      if (/\bbox-shadow\b|\bdrop-shadow\b/.test(line)) add('shadow', 'Shadows are banned. Use 1 px borders and surface steps.');
      if (/\bbackdrop-filter\b/.test(line)) add('blur', 'Backdrop blur (liquid glass) is banned.');
      if (/\b(linear|radial|conic)-gradient\b/.test(line)) add('gradient', 'Gradients are banned unless approved in an ADR.');
      if (/font-family\s*:[^;]*\b(Inter|Geist|Space Grotesk)\b/i.test(line)) add('font', 'Banned font family.');
      const radius = /border-radius\s*:\s*([^;}]+)/.exec(line);
      if (radius && radiusTooLarge(radius[1])) add('radius', `border-radius above ${MAX_RADIUS_PX}px is banned.`);
      if (!isTokens) {
        if (/:[^;{}]*\bwhite\b/i.test(line)) add('white', 'Pure white is banned. Use design tokens.');
        if (/(?<![\w&])#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![\w-])/.test(line)
          && !/href=|url\(#/.test(line)) {
          add('raw-hex', 'Raw hex colors are only allowed in tokens.css.');
        }
      } else if (/#fff(fff)?(?![0-9a-fA-F])/i.test(line)) {
        add('white', 'Pure white (#FFFFFF) is banned, even in tokens.');
      }
    }

    if (isCode && /from\s+['"][^'"]*lucide[^'"]*['"]/i.test(line)) add('lucide', 'Lucide icons are banned. Use Phosphor Regular.');
    if (e === '.json' && p.endsWith('package.json') && /"[^"]*lucide[^"]*"\s*:/i.test(line)) add('lucide', 'Lucide packages are banned.');
  });
  return findings;
}
