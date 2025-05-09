import {spawnSync} from 'child_process';
import {resolve} from 'path';

process.env.YARN_SWITCH_DEFAULT = `local:${resolve(import.meta.dirname, `../target/release/zpm`)}`;

const releaseBinary = resolve(import.meta.dirname, `../target/release/zpm-switch`);

process.exitCode = spawnSync(releaseBinary, process.argv.slice(2), {
  stdio: `inherit`,
}).status;
