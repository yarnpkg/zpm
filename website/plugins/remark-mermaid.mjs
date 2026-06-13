import {readFileSync}  from 'fs';
import {createRequire} from 'module';
import {visit}         from 'unist-util-visit';

const require = createRequire(import.meta.url);
const mermaidJs = readFileSync(require.resolve(`mermaid/dist/mermaid.min.js`), `utf-8`);

let _browser;
async function getBrowser() {
  if (!_browser) {
    const puppeteer = await import(`puppeteer`);
    _browser = await puppeteer.default.launch();
  }
  return _browser;
}

let _counter = 0;

async function renderMermaid(code) {
  const browser = await getBrowser();
  const page = await browser.newPage();

  try {
    await page.setContent(`<html><body></body></html>`);
    await page.addScriptTag({content: mermaidJs});

    const baseId = `m${++_counter}`;

    return await page.evaluate(async (code, baseId) => {
      const svgs = {};
      for (const [key, theme] of [[`dark`, `dark`], [`light`, `default`]]) {
        document.body.innerHTML = ``;
        window.mermaid.initialize({startOnLoad: false, look: `handDrawn`, theme});
        const {svg} = await window.mermaid.render(baseId + key[0], code);
        svgs[key] = svg;
      }
      return svgs;
    }, code, baseId);
  } finally {
    await page.close();
  }
}

export default function remarkMermaid() {
  return async tree => {
    const nodes = [];

    visit(tree, `code`, (node, index, parent) => {
      if (!parent || node.lang !== `mermaid`) return;
      nodes.push({node, index, parent});
    });

    if (!nodes.length) return;

    await Promise.all(nodes.map(async ({node, index, parent}) => {
      const {dark, light} = await renderMermaid(node.value);

      parent.children[index] = {
        type: `html`,
        value: [
          `<div class="mermaid-diagram">`,
          `<div class="mermaid-dark">${dark}</div>`,
          `<div class="mermaid-light">${light}</div>`,
          `</div>`,
        ].join(`\n`),
      };
    }));
  };
}
