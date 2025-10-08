import reactEslintConfig from '@yarnpkg/eslint-config/react';
import eslintConfig      from '@yarnpkg/eslint-config';

// eslint-disable-next-line arca/no-default-export
export default [
  {
    ignores: [
      `.pnp.*`,
      `.yarn/**`,
      `**/*.rs`,
      `documentation/.astro`,
      `packages/zpm/src/constraints/constraints.tpl.js`,
    ],
  },
  ...eslintConfig,
  ...reactEslintConfig,
  {
    files: [
      `documentation/src/**/*.tsx`,
    ],
    rules: {
      [`arca/no-default-export`]: `off`,
    },
  },
];
