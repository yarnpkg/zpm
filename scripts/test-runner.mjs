import p               from 'child_process';
import {createRequire} from 'module';

const require = createRequire(import.meta.url);

const result = p.spawnSync(require.resolve(`../target/release/zpm`), process.argv.slice(2), {
  stdio: `inherit`,
});

process.exitCode = result.status;
