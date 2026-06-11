import {spawn}                    from 'node:child_process';
import {writeFileSync, mkdirSync} from 'node:fs';
import {platform}                 from 'node:os';
import {resolve, dirname}         from 'node:path';

const SGR_MAP: Record<number, string | null> = {
  0: null,
  2: `dim`,
  31: `err`, 91: `err`,
  32: `ok`, 92: `ok`,
  33: `warn`, 93: `warn`,
  34: `accent`, 94: `accent`,
  35: `accent`, 95: `accent`,
  36: `accent`, 96: `accent`,
};

function escapeHtml(s: string): string {
  return s.replace(/&/g, `&amp;`).replace(/</g, `&lt;`).replace(/>/g, `&gt;`);
}

function ansiToHtml(line: string): string {
  const parts: Array<string> = [];
  let cls: string | null = null;
  let last = 0;

  const re = /\x1b\[([\d;]*)m/g;
  let m: RegExpExecArray | null;

  while ((m = re.exec(line)) !== null) {
    const text = line.slice(last, m.index);
    if (text)
      parts.push(cls ? `<span class="${cls}">${escapeHtml(text)}</span>` : escapeHtml(text));


    last = m.index + m[0].length;

    const codes = m[1] ? m[1].split(`;`).map(Number) : [0];
    for (const code of codes) {
      if (code in SGR_MAP) {
        cls = SGR_MAP[code];
      }
    }
  }

  const tail = line.slice(last);
  if (tail)
    parts.push(cls ? `<span class="${cls}">${escapeHtml(tail)}</span>` : escapeHtml(tail));


  return parts.join(``);
}

function stripControl(s: string): string {
  s = s
    .replace(/\x1b\[[\d;]*[A-HJKSTfhlnr]/g, ``)
    .replace(/\x1b\[\?[\d;]*[a-zA-Z]/g, ``)
    .replace(/\x1b[()][AB012]/g, ``)
    .replace(/\x1b[>=]/g, ``);

  while (s.includes(`\x08`)) {
    s = s.replace(/[^\x08]\x08/, ``);
    s = s.replace(/^\x08+/, ``);
  }

  return s.replace(/[\x00-\x07\x0e-\x1a\x1c-\x1f\x7f]/g, ``);
}

const ddIdx = process.argv.indexOf(`--`);
if (ddIdx < 0 || ddIdx + 1 >= process.argv.length) {
  console.error(`Usage: node record-terminal.ts [<id>] -- <command> [args...]`);
  process.exit(1);
}

const preArgs = process.argv.slice(2, ddIdx);
const terminalId = preArgs[0] ?? null;

const args = process.argv.slice(ddIdx + 1);
const cmd = args[0];
const cmdArgs = args.slice(1);

type Entry = {html: string, delay: number, clear?: number};
const entries: Array<Entry> = [];

entries.push({
  html: `<span class="prompt">$</span> <span class="cmd">${escapeHtml(args.join(` `))}</span>`,
  delay: 0,
});

const env: Record<string, string | undefined> = {...process.env, FORCE_COLOR: `3`, CLICOLOR_FORCE: `1`};
delete env.NO_COLOR;

let spawnCmd: string;
let spawnArgs: Array<string>;

if (platform() === `darwin`) {
  spawnCmd = `script`;
  spawnArgs = [`-q`, `/dev/null`, cmd, ...cmdArgs];
} else if (platform() === `linux`) {
  const quoted = [cmd, ...cmdArgs].map(a => `'${a.replace(/'/g, `'\\''`)}'`).join(` `);
  spawnCmd = `script`;
  spawnArgs = [`-qc`, quoted, `/dev/null`];
} else {
  spawnCmd = cmd;
  spawnArgs = cmdArgs;
}

const child = spawn(spawnCmd, spawnArgs, {
  stdio: [`inherit`, `pipe`, `pipe`],
  env,
});

let buf = ``;
let lastTime = performance.now();
let afterCR = false;
const BASE_DELAY = 300;

function emitLine(raw: string) {
  const cleaned = stripControl(raw);
  const html = ansiToHtml(cleaned);
  const delay = entries.length === 1 ? BASE_DELAY : Math.round(performance.now() - lastTime);

  const entry: Entry = {html, delay};
  if (afterCR) entry.clear = 1;

  entries.push(entry);
  lastTime = performance.now();
  afterCR = false;
}

function flush(chunk: string) {
  buf += chunk;

  buf = buf.replace(/\x1b\[2K/g, `\r`);

  let pos = 0;
  while (pos < buf.length) {
    const rIdx = buf.indexOf(`\r`, pos);
    const nIdx = buf.indexOf(`\n`, pos);

    if (rIdx === -1 && nIdx === -1) break;

    if (rIdx !== -1 && (nIdx === -1 || rIdx < nIdx)) {
      if (rIdx + 1 >= buf.length) break;

      const text = buf.slice(pos, rIdx);

      if (buf[rIdx + 1] === `\n`) {
        if (text) emitLine(text);
        pos = rIdx + 2;
      } else {
        if (text) emitLine(text);
        afterCR = true;
        pos = rIdx + 1;
      }
    } else {
      const text = buf.slice(pos, nIdx);
      if (text) emitLine(text);
      pos = nIdx + 1;
    }
  }

  buf = buf.slice(pos);
}

child.stdout.on(`data`, (d: Buffer) => flush(d.toString()));
child.stderr.on(`data`, (d: Buffer) => flush(d.toString()));

child.on(`close`, () => {
  if (buf.length > 0) {
    const cleaned = stripControl(buf);
    const html = ansiToHtml(cleaned);
    const delay = entries.length === 1 ? BASE_DELAY : Math.round(performance.now() - lastTime);
    const entry: Entry = {html, delay};
    if (afterCR) entry.clear = 1;
    entries.push(entry);
  }

  const json = `${JSON.stringify(entries, null, 2)}\n`;

  if (terminalId) {
    const outPath = resolve(import.meta.dirname!, `../src/data/terminals/${terminalId}.json`);
    mkdirSync(dirname(outPath), {recursive: true});
    writeFileSync(outPath, json);
    console.error(`Wrote ${entries.length} entries to ${outPath}`);
  } else {
    process.stdout.write(json);
  }
});
