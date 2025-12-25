export type Caller = {
  file: string | null;
  methodName: string;
  arguments: Array<string>;
  line: number | null;
  column: number | null;
};

const chromeRe = /^\s*at (.*?) ?\(((?:file|https?|blob|chrome-extension|native|eval|webpack|<anonymous>|\/|[a-z]:\\|\\\\).*?)(?::(\d+))?(?::(\d+))?\)?\s*$/i;
const chromeEvalRe = /\((\S*)(?::(\d+))(?::(\d+))\)/;

// https://github.com/errwischt/stacktrace-parser/blob/f70768a12579de3469f3fdfdc423657ee6609c7c/src/stack-trace-parser.js
function parseStackLine(line: string): Caller | null {
  const parts = chromeRe.exec(line) as [string, string, string, string, string] | null;
  if (!parts)
    return null;

  const isNative = parts[2] && parts[2].indexOf(`native`) === 0; // start of line
  const isEval = parts[2] && parts[2].indexOf(`eval`) === 0; // start of line

  const submatch = chromeEvalRe.exec(parts[2]);
  if (isEval && submatch != null) {
    // throw out eval line/column and use top-most line/column number
    parts[2] = submatch[1]!; // url
    parts[3] = submatch[2]!; // line
    parts[4] = submatch[3]!; // column
  }

  return {
    file: !isNative ? parts[2] : null,
    methodName: parts[1] || `<unknown>`,
    arguments: isNative ? [parts[2]] : [],
    line: parts[3] ? +parts[3] : null,
    column: parts[4] ? +parts[4] : null,
  };
}

export function getCaller() {
  const err = new Error();
  const line = err.stack!.split(`\n`)[3]!;

  return parseStackLine(line);
}
