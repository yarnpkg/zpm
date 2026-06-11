import {readdirSync, readFileSync, mkdirSync, existsSync} from 'node:fs';
import {createServer}                                     from 'node:http';
import {resolve, dirname, relative, extname, join}        from 'node:path';
import {fileURLToPath}                                    from 'node:url';
import puppeteer                                          from 'puppeteer';

const __dirname = fileURLToPath(new URL(`.`, import.meta.url));
const distDir = resolve(__dirname, `..`, `dist`);
const ogDir = join(distDir, `og`);
const CONCURRENCY = 4;
const WIDTH = 1200;
const HEIGHT = 630;

function collectHtmlFiles(dir: string, base: string = dir): Array<string> {
  const entries = readdirSync(dir, {withFileTypes: true});
  const files: Array<string> = [];

  for (const entry of entries) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectHtmlFiles(full, base));
    } else if (entry.name.endsWith(`.html`)) {
      const rel = relative(base, full);
      if (!rel.startsWith(`presentation/`)) {
        files.push(rel);
      }
    }
  }

  return files;
}

function startStaticServer(root: string): Promise<{url: string, close: () => void}> {
  return new Promise(resolve => {
    const mimeTypes: Record<string, string> = {
      '.html': `text/html`,
      '.css': `text/css`,
      '.js': `application/javascript`,
      '.json': `application/json`,
      '.png': `image/png`,
      '.jpg': `image/jpeg`,
      '.svg': `image/svg+xml`,
      '.woff2': `font/woff2`,
      '.woff': `font/woff`,
    };

    const server = createServer((req, res) => {
      const url = new URL(req.url!, `http://localhost`);
      let filePath = join(root, url.pathname);

      if (!existsSync(filePath) || !filePath.includes(`.`)) {
        const withHtml = filePath.endsWith(`/`)
          ? join(filePath, `index.html`)
          : `${filePath}.html`;
        if (existsSync(withHtml)) {
          filePath = withHtml;
        }
      }

      if (!existsSync(filePath)) {
        res.writeHead(404);
        res.end();
        return;
      }

      const ext = extname(filePath);
      res.writeHead(200, {'Content-Type': mimeTypes[ext] ?? `application/octet-stream`});
      res.end(readFileSync(filePath));
    });

    server.listen(0, `127.0.0.1`, () => {
      const addr = server.address() as {port: number};
      resolve({url: `http://127.0.0.1:${addr.port}`, close: () => server.close()});
    });
  });
}

async function run() {
  const htmlFiles = collectHtmlFiles(distDir);
  console.log(`Found ${htmlFiles.length} pages to screenshot`);

  const server = await startStaticServer(distDir);
  const browser = await puppeteer.launch({headless: true});

  let completed = 0;

  async function screenshot(htmlFile: string) {
    const route = htmlFile
      .replace(/\.html$/, ``)
      .replace(/\/index$/, ``);

    const pagePath = route || `index`;
    const outPath = join(ogDir, `${pagePath}.png`);

    mkdirSync(dirname(outPath), {recursive: true});

    const page = await browser.newPage();
    await page.setViewport({width: WIDTH, height: HEIGHT});

    const url = `${server.url}/${htmlFile}`;
    await page.goto(url, {waitUntil: `networkidle2`, timeout: 30_000});

    await page.screenshot({path: outPath, type: `png`});
    await page.close();

    completed++;
    if (completed % 10 === 0 || completed === htmlFiles.length) {
      console.log(`  ${completed}/${htmlFiles.length}`);
    }
  }

  const queue = [...htmlFiles];
  async function worker() {
    while (queue.length > 0) {
      const file = queue.shift()!;
      await screenshot(file);
    }
  }

  await Promise.all(Array.from({length: CONCURRENCY}, () => worker()));

  await browser.close();
  server.close();

  console.log(`Generated ${completed} OG images in dist/og/`);
}

run().catch(err => {
  console.error(err);
  process.exit(1);
});
