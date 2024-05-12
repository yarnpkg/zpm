import p               from 'child_process';
import fs              from 'fs';
import {createRequire} from 'module';
import os              from 'os';
import path            from 'path';

const require = createRequire(import.meta.url);

const berryPath = process.env.BERRY_PATH || path.join(os.homedir(), `berry`);

const child = p.spawnSync(`yarn`, [`test:integration`, `--json`, ...process.argv.slice(2)], {
  cwd: berryPath,
  encoding: `utf8`,
  maxBuffer: 1024 * 1024 * 1024,
  stdio: [
    `ignore`,
    `pipe`,
    `inherit`,
  ],
  env: {
    ...process.env,
    TEST_BINARY: require.resolve(`./test-runner.mjs`),
  },
});

const result = JSON.parse(child.stdout);
const resultPath = require.resolve(`../test-results.md`);

const statusKind = {
  passed: {
    tag: `<!-- test:passed -->`,
    img: `https://img.shields.io/badge/passed-green`,
  },
  failed: {
    tag: `<!-- test:failed -->`,
    img: `https://img.shields.io/badge/failed-red`,
  },
};

const statusList = Object.entries(statusKind);

const testSuites = {};

let testSuite;

for (const line of fs.readFileSync(resultPath, `utf8`).split(/\n/)) {
  if (line.startsWith(`<!-- test:suite -->`)) {
    testSuite = line.slice(19);
  } else {
    for (const [status, {tag}] of statusList) {
      if (line.startsWith(tag)) {
        testSuites[testSuite] ??= {};
        testSuites[testSuite][line.slice(tag.length)] = status;
        break;
      }
    }
  }
}

for (const testSuite of result.testResults) {
  const fileName = path.relative(berryPath, testSuite.name);

  for (const test of testSuite.assertionResults) {
    if (!Object.prototype.hasOwnProperty.call(statusKind, test.status))
      continue;

    testSuites[fileName] ??= {};
    testSuites[fileName][test.fullName] = test.status;
  }
}

const sortedTestSuites = Object.entries(testSuites).sort((a, b) => a[0].localeCompare(b[0])).flatMap(([testSuite, tests]) => {
  const sortedTests = Object.entries(tests).sort((a, b) => a[0].localeCompare(b[0]));
  if (sortedTests.length === 0)
    return [];

  return [[testSuite, sortedTests]];
});

let output = `# Test Results\n\n`;

for (const [testSuite, tests] of sortedTestSuites) {
  for (const [name, status] of tests) {
    output += `![](./scripts/${status}.png) `;
  }
}

output += `\n\n<table>\n`;

for (const [testSuite, tests] of sortedTestSuites) {
  output += `\n<tr><th colspan=2>\n\n<!-- test:suite -->${testSuite}\n\n</th></tr><tr><td colspan=2>\n\n`;

  for (const [name, status] of tests) {
    output += `![](./scripts/${status}.png) `;
  }

  output += `\n\n</td></tr>`;

  for (const [name, status] of tests) {
    output += `<tr><td><img src="${statusKind[status].img}"/></td><td>\n\n${statusKind[status].tag}${name}\n</td></tr>\n`;
  }
}

output += `</table>\n`;

fs.writeFileSync(resultPath, output);
