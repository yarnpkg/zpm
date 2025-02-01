import {spawnSync} from 'child_process';
import {resolve} from 'path';

const releaseBinary = resolve(import.meta.dirname, `../target/release/zpm`);

process.env.YES_I_KNOW_THIS_IS_EXPERIMENTAL = '1';

process.exitCode = spawnSync(releaseBinary, process.argv.slice(2), {
  stdio: `inherit`,
}).status;
