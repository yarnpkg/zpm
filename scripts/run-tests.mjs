import p               from 'child_process';
import fs              from 'fs';
import {createRequire} from 'module';
import os              from 'os';
import path            from 'path';

const require = createRequire(import.meta.url);

const berryPath = process.env.BERRY_PATH || path.join(os.homedir(), `berry`);
const txtPath = require.resolve(`../test-results.txt`);
const overridesPath = require.resolve(`../test-overrides.txt`);

const overrides = new Set(fs.readFileSync(overridesPath, `utf8`).split(/\n/).filter(line => {
  return line.length > 0 && line[0] !== ` `;
}));

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

const kindToChar = {
  passed: `🟩`,
  failed: `🟥`,
  skipped: `🟦`,
};

const charToKind = {
  [`🟩`]: `passed`,
  [`🟥`]: `failed`,
  [`🟦`]: `skipped`,
};

const testSuites = {};
let activeTestSuite;

for (const line of fs.readFileSync(txtPath, `utf8`).split(/\n/)) {
  if (!line)
    continue;

  const firstChar = [...line][0];

  if (Object.prototype.hasOwnProperty.call(charToKind, firstChar)) {
    testSuites[activeTestSuite] ??= {};
    testSuites[activeTestSuite][line.slice(firstChar.length + 1)] = charToKind[firstChar];
  } else {
    activeTestSuite = line.replace(/^\[[^\]]*\] */, ``);
  }
}

for (const testSuite of result.testResults) {
  const fileName = path.relative(berryPath, testSuite.name);

  for (const test of testSuite.assertionResults) {
    if (!Object.prototype.hasOwnProperty.call(kindToChar, test.status))
      continue;

    const fullName = test.ancestorTitles.concat(test.title).join(` ► `);

    const status = overrides.has(fullName)
      ? test.status === `failed` ? `skipped` : `failed`
      : test.status;

    testSuites[fileName] ??= {};
    testSuites[fileName][fullName] = status;
  }
}

const sortedTestSuites = Object.entries(testSuites).sort((a, b) => a[0].localeCompare(b[0])).flatMap(([testSuite, tests]) => {
  const sortedTests = Object.entries(tests).sort((a, b) => a[0].localeCompare(b[0]));
  if (sortedTests.length === 0)
    return [];

  return [[testSuite, sortedTests]];
});

function generateTxt() {
  let txt = ``;

  for (const [testSuite, tests] of sortedTestSuites) {
    const passingTests = tests.filter(([, status]) => status === `passed`).length;
    const totalTests = tests.length;

    txt += `[${passingTests} / ${totalTests}] ${testSuite}\n\n`;

    for (const [name, status] of tests) {
      txt += `${kindToChar[status]} ${name}\n`;
    }

    txt += `\n`;
  }

  return txt.slice(0, -1);
}

//fs.writeFileSync(mdPath, generateMd());
fs.writeFileSync(txtPath, generateTxt());
