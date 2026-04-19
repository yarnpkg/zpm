import {scopedPreflightStyles, isolateInsideOfContainer} from 'tailwindcss-scoped-preflight';

// eslint-disable-next-line arca/no-default-export
export default scopedPreflightStyles({
  isolationStrategy: isolateInsideOfContainer(`.markdown-body`),
});
