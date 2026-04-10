#!/usr/bin/env node

const fs = require(`node:fs`);
const path = require(`node:path`);

type TestStatus = {
  test: string;
  exitCode: number;
};

type GithubIssue = {
  number: number;
  title: string;
};

const ISSUE_LABEL = `e2e-failure`;

function requiredEnv(name: string): string {
  const value = process.env[name];
  if (!value)
    throw new Error(`Missing required environment variable: ${name}`);
  return value;
}

function findFilesRecursively(root: string, filename: string): Array<string> {
  if (!fs.existsSync(root))
    return [];

  const matches: Array<string> = [];
  const queue: Array<string> = [root];

  while (queue.length > 0) {
    const current = queue.shift()!;
    const entries = fs.readdirSync(current, {withFileTypes: true});

    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        queue.push(fullPath);
      } else if (entry.isFile() && entry.name === filename) {
        matches.push(fullPath);
      }
    }
  }

  return matches;
}

function sanitizeLogTail(logFilePath: string): string {
  if (!fs.existsSync(logFilePath))
    return `Log file not found at ${logFilePath}`;

  const text = fs.readFileSync(logFilePath, `utf8`);
  const lines = text.split(/\r?\n/).slice(-200);
  const tail = lines.join(`\n`).trim();

  if (tail.length === 0)
    return `Log file is empty`;

  // Keep enough context while staying well under issue body limits.
  return tail.slice(-30000);
}

async function githubRequest<T = any>(
  token: string,
  repository: string,
  method: string,
  urlPath: string,
  body: unknown = undefined,
): Promise<T> {
  const response = await fetch(`https://api.github.com${urlPath}`, {
    method,
    headers: {
      authorization: `Bearer ${token}`,
      accept: `application/vnd.github+json`,
      'x-github-api-version': `2022-11-28`,
      'user-agent': `${repository}-e2e-reporter`,
      ...(body === undefined ? {} : {'content-type': `application/json`}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`GitHub API ${method} ${urlPath} failed (${response.status}): ${text}`);
  }

  if (response.status === 204)
    return undefined as T;

  return await response.json() as T;
}

async function ensureLabel(token: string, owner: string, repo: string) {
  try {
    await githubRequest(token, `${owner}/${repo}`, `POST`, `/repos/${owner}/${repo}/labels`, {
      name: ISSUE_LABEL,
      color: `b60205`,
      description: `Automated end-to-end test failures`,
    });
  } catch (error: any) {
    if (!String(error.message).includes(`422`)) {
      throw error;
    }
  }
}

async function getOpenE2EIssues(token: string, owner: string, repo: string): Promise<Array<GithubIssue>> {
  const issues = await githubRequest<Array<GithubIssue>>(
    token,
    `${owner}/${repo}`,
    `GET`,
    `/repos/${owner}/${repo}/issues?state=open&labels=${encodeURIComponent(ISSUE_LABEL)}&per_page=100`,
  );

  return issues.filter(issue => issue.title.startsWith(`E2E failure:`));
}

function buildIssueBody(testStatus: TestStatus, logTail: string): string {
  const runUrl = `${requiredEnv(`GITHUB_SERVER_URL`)}/${requiredEnv(`GITHUB_REPOSITORY`)}/actions/runs/${requiredEnv(`GITHUB_RUN_ID`)}`;
  const sha = requiredEnv(`GITHUB_SHA`);
  const ref = requiredEnv(`GITHUB_REF_NAME`);

  return [
    `Automated e2e test \`${testStatus.test}\` failed.`,
    ``,
    `- Workflow run: ${runUrl}`,
    `- Commit: ${sha}`,
    `- Ref: ${ref}`,
    `- Exit code: ${testStatus.exitCode}`,
    ``,
    `### Log tail`,
    ``,
    `\`\`\`text`,
    logTail,
    `\`\`\``,
  ].join(`\n`);
}

async function main() {
  const artifactRoot = process.env.E2E_ARTIFACTS_DIR ?? `e2e-artifacts`;
  const statusFiles = findFilesRecursively(artifactRoot, `status.json`);

  if (statusFiles.length === 0) {
    console.log(`No status files found under ${artifactRoot}; skipping issue reporting.`);
    return;
  }

  const statuses = statusFiles.map(statusFile => {
    const status = JSON.parse(fs.readFileSync(statusFile, `utf8`)) as TestStatus;
    const logFile = path.join(path.dirname(statusFile), `run.log`);
    return {
      status,
      logFile,
    };
  });

  const failingStatuses = statuses.filter(entry => entry.status.exitCode !== 0);

  if (failingStatuses.length === 0) {
    console.log(`All e2e tests passed; no issues to create or update.`);
    return;
  }

  const token = requiredEnv(`GITHUB_TOKEN`);
  const repository = requiredEnv(`GITHUB_REPOSITORY`);
  const [owner, repo] = repository.split(`/`);
  if (!owner || !repo)
    throw new Error(`Invalid GITHUB_REPOSITORY value: ${repository}`);

  await ensureLabel(token, owner, repo);
  const openIssues = await getOpenE2EIssues(token, owner, repo);

  for (const {status, logFile} of failingStatuses) {
    const title = `E2E failure: ${status.test}`;
    const body = buildIssueBody(status, sanitizeLogTail(logFile));
    const existing = openIssues.find(issue => issue.title === title);

    if (existing) {
      await githubRequest(
        token,
        `${owner}/${repo}`,
        `PATCH`,
        `/repos/${owner}/${repo}/issues/${existing.number}`,
        {body},
      );
      console.log(`Updated issue #${existing.number}: ${title}`);
    } else {
      const created = await githubRequest<GithubIssue>(
        token,
        `${owner}/${repo}`,
        `POST`,
        `/repos/${owner}/${repo}/issues`,
        {
          title,
          body,
          labels: [ISSUE_LABEL],
        },
      );
      console.log(`Created issue #${created.number}: ${title}`);
    }
  }
}

main().catch((error: Error) => {
  console.error(error.stack ?? error.message);
  process.exit(1);
});
