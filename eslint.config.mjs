// @ts-check
import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import pluginVue from 'eslint-plugin-vue';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: [
      '**/node_modules/',
      '**/dist/',
      '**/target/',
      '**/coverage/',
      'apps/desktop/src-tauri/gen/',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.strict,
  ...pluginVue.configs['flat/recommended'],
  {
    files: ['**/*.vue'],
    languageOptions: {
      parserOptions: { parser: tseslint.parser },
    },
  },
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
  },
  // Must stay last: turns off rules that conflict with Prettier.
  prettier,
);
