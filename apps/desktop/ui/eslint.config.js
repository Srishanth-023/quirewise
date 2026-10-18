import js from '@eslint/js';
import globals from 'globals';
import svelte from 'eslint-plugin-svelte';
import svelteParser from 'svelte-eslint-parser';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['dist/', 'node_modules/', 'src/lib/ipc/generated.ts'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...svelte.configs['flat/recommended'],
  {
    languageOptions: { globals: { ...globals.browser } },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/no-non-null-assertion': 'error',
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            { group: ['lucide*', '@lucide/*'], message: 'Lucide icons are banned. Use Phosphor Regular.' },
          ],
        },
      ],
    },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: { parser: svelteParser, parserOptions: { parser: tseslint.parser } },
  },
  {
    files: ['**/*.js', '**/*.mjs'],
    languageOptions: { globals: { ...globals.node } },
  },
);
