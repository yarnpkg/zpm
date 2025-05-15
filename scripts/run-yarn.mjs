import {spawnSync} from 'child_process';
import {resolve} from 'path';

const zpmBuild = process.env.TEST_BUILD ?? `release`;

const zpmSwitchBinaryPath = resolve(import.meta.dirname, `../target/${zpmBuild}/yarn-switch`);
const zpmBinaryPath = resolve(import.meta.dirname, `../target/${zpmBuild}/yarn`);

// So that Yarn Switch knows it should load a local binary; this requires
// that the repository doesn't have a `packageManager` field set up.
process.env.YARNSW_DEFAULT = `local:${zpmBinaryPath}`;

// So that the test runner from the Yarn Berry repository knows where to
// find the zpm test binary. It needs to be a JS file.
process.env.TEST_BINARY = import.meta.filename;

process.exitCode = spawnSync(zpmSwitchBinaryPath, process.argv.slice(2), {
  stdio: `inherit`,
}).status;
