import {optimizer, tooltip}      from '@clipanion/expressive-code/extra';
import {clipanionExpressiveCode} from '@clipanion/expressive-code';
import {createRequire}           from 'module';

const require = createRequire(import.meta.url);

// eslint-disable-next-line arca/no-default-export
export default {
  plugins: [
    tooltip(),
    optimizer(),
    clipanionExpressiveCode({
      clis: {
        [`yarn`]: {
          baseUrl: `https://yarn6.netlify.app/reference/cli`,
          path: require.resolve(`@yarnpkg/monorepo/target/release/yarn-bin`),
        },
      },
    }),
  ],
};
