import {createWorkspace} from './workspace';

const workspace = createWorkspace({
  cwd: '/workspace',
  packageManager: process.env.npm_config_user_agent ?? 'yarn',
});

await workspace.install();
console.log(await workspace.explain('react'));
