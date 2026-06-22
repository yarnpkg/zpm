import reactEslintConfig from '@yarnpkg/eslint-config/react';
import eslintConfig      from '@yarnpkg/eslint-config';

const browserGlobals = {
  customElements: `readonly`,
  document: `readonly`,
  HTMLElement: `readonly`,
  history: `readonly`,
  localStorage: `readonly`,
  location: `readonly`,
  requestAnimationFrame: `readonly`,
  window: `readonly`,
};

// eslint-disable-next-line arca/no-default-export
export default [
  {
    ignores: [
      `.pnp.*`,
      `.yarn/**`,
      `**/*.rs`,
      `**/dist`,
      `tests/acceptance-tests/pkg-tests-fixtures`,
      `website/.astro`,
      `packages/zpm/src/constraints/constraints.tpl.js`,
      `**/generated/**`,
      `**/*.generated.ts`,
    ],
  },
  ...eslintConfig,
  ...reactEslintConfig,
  {
    files: [
      `website/**/*.tsx`,
    ],
    rules: {
      [`arca/no-default-export`]: `off`,
    },
  },
  {
    files: [
      `website/astro.config.mjs`,
      `website/plugins/**/*.mjs`,
    ],
    rules: {
      [`arca/no-default-export`]: `off`,
    },
  },
  {
    files: [
      `website/public/**/*.js`,
    ],
    languageOptions: {
      globals: {
        ...browserGlobals,
        LEVELS: `readonly`,
        QUESTIONS: `readonly`,
      },
    },
  },
  {
    files: [
      `website/plugins/remark-mermaid.mjs`,
    ],
    languageOptions: {
      globals: browserGlobals,
    },
  },
  {
    files: [
      `website/scripts/record-terminal.ts`,
      `website/src/components/package/utils.ts`,
    ],
    rules: {
      [`no-control-regex`]: `off`,
    },
  },
  {
    files: [`tests/acceptance-tests/pkg-tests-specs/**/*.test.{js,ts}`],
    languageOptions: {
      globals: {
        makeTemporaryEnv: `readonly`,
        makeTemporaryMonorepoEnv: `readonly`,
      },
    },
  },
];
