import {createHighlighter, createCssVariablesTheme} from 'shiki';

const cssVarsTheme = createCssVariablesTheme({
  name: `css-variables`,
  variablePrefix: `--shiki-`,
  variableDefaults: {},
  fontStyle: true,
});

let highlighter: Awaited<ReturnType<typeof createHighlighter>> | undefined;

async function getHighlighter() {
  if (highlighter) return highlighter;

  highlighter = await createHighlighter({
    themes: [cssVarsTheme],
    langs: [`javascript`, `typescript`, `json`, `yaml`, `bash`, `html`, `css`, `jsx`, `tsx`, `diff`, `shell`],
  });

  return highlighter;
}

const LANG_ALIASES: Record<string, string> = {
  js: `javascript`,
  ts: `typescript`,
  sh: `shell`,
};

export async function highlight(code: string, lang: string): Promise<string> {
  if (!lang) return escapeHtml(code);

  const resolved = LANG_ALIASES[lang] || lang;
  const hl = await getHighlighter();

  const loaded = hl.getLoadedLanguages();
  if (!loaded.includes(resolved)) return escapeHtml(code);

  const html = hl.codeToHtml(code, {
    lang: resolved,
    theme: `css-variables`,
  });

  const match = html.match(/<code>(.+?)<\/code>/s);
  return match ? match[1] : escapeHtml(code);
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, `&amp;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`)
    .replace(/"/g, `&quot;`);
}
