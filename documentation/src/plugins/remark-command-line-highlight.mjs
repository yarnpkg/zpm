import {getCli}       from '@yarnpkg/cli';
import {parseShell}   from '@yarnpkg/parsers';
import {capitalize}   from 'es-toolkit/compat';
import {visit}        from 'unist-util-visit';

import manifestSchema from '../utils/configuration/manifest.json';
import yarnrcSchema   from '../utils/configuration/yarnrc.json';

let cliData = null;
let userSpecs = null;

export function remarkCommandLineHighlight() {
  return async tree => {
    if (!cliData) {
      try {
        cliData = await getCli();
      } catch {
        return;
      }
    }

    if (!userSpecs) {
      try {
        userSpecs = [
          {
            schema: manifestSchema,
            urlGenerator: name =>
              `/configuration/manifest${
                process.env.NODE_ENV === `development` ? `` : `/`
              }#${name}`,
          },
          {
            schema: yarnrcSchema,
            urlGenerator: name =>
              `/configuration/yarnrc${
                process.env.NODE_ENV === `development` ? `` : `/`
              }#${name}`,
          },
        ];
      } catch {
        userSpecs = null;
      }
    }

    visit(tree, `code`, node => handleCodeBlock(node, cliData));
    visit(tree, `inlineCode`, node => {
      handleCodeBlock(node, cliData);

      // Try to link configuration options
      if (userSpecs) {
        const match = node.value.match(/^(?<name>[^:]+)(?:: (?<value>.*))?$/);
        if (match) {
          const segments = match.groups.name.split(`.`);

          let result = null;

          userSpecs.find(spec => {
            let schemaNode = spec.schema;

            for (const segment of segments) {
              if (
                schemaNode &&
                typeof schemaNode === `object` &&
                schemaNode.properties
              ) {
                if (!Object.hasOwn(schemaNode.properties, segment))
                  return false;

                schemaNode = schemaNode.properties[segment];
              } else {
                return false;
              }
            }

            if (!schemaNode || typeof schemaNode.title === `undefined`)
              return false;

            result = {
              title: schemaNode.title,
              description: schemaNode.description || ``,
              url: spec.urlGenerator(segments.join(`.`)),
            };

            return true;
          });

          if (result !== null) {
            const attributes = {...match.groups, ...result};

            const escapedTooltip = escapeHtmlAttribute(attributes.title);
            const escapedName = escapeHtml(attributes.name);

            node.type = `html`;
            node.value = `<a href="${attributes.url}" class="tooltip-link text-white/80!" aria-label="${escapedTooltip}" data-tooltip="${escapedTooltip}">${escapedName}</a>`;
            return;
          }
        }
      }
    });
  };
}

function handleCodeBlock(node, cliData) {
  const isInlineCode = node.type === `inlineCode`;

  const lines = node.value
    .trim()
    .split(`\n`)
    .map(line => {
      if (line.startsWith(`#`) || line.length === 0) {
        return makeRawLine(line);
      } else if (
        line.startsWith(`${cliData.binaryName} `) ||
        line === cliData.binaryName
      ) {
        return makeCommandOrRawLine(line, cliData);
      } else {
        return null;
      }
    });

  // If any line can't be processed, bail out
  if (lines.some(line => line === null)) return;

  const htmlContent = lines
    .map(line => {
      if (line.type === `raw`) {
        return escapeHtml(line.value);
      } else {
        return renderCommandLine(line);
      }
    })
    .join(isInlineCode ? ` ` : `<br />`);

  node.type = `html`;

  if (isInlineCode) {
    node.value = `<code>${htmlContent}</code>`;
  } else {
    node.value = `<div class="custom-code-block">${htmlContent}</div>`;
  }
}

function makeRawLine(line) {
  return {
    type: `raw`,
    value: line,
  };
}

function makeCommandLine(line, cli) {
  const parsed = parseShell(line)[0].command.chain;

  if (parsed.type !== `command`) {
    throw new Error(
      `Unsupported command type: "${parsed.type}" when parsing "${line}"`,
    );
  }

  const strArgs = parsed.args.map(arg => {
    if (arg.type !== `argument`) {
      throw new Error(
        `Unsupported argument type: "${arg.type}" when parsing "${line}"`,
      );
    }

    if (arg.segments.length !== 1) {
      throw new Error(
        `Unsupported argument segments length: "${arg.segments.length}" when parsing "${line}"`,
      );
    }

    const segment = arg.segments[0];
    if (segment.type !== `text`) {
      throw new Error(
        `Unsupported argument segment type: "${segment.type}" when parsing "${line}"`,
      );
    }

    return segment.text;
  });

  const [name, ...argv] = strArgs;
  const splitPoint = argv.indexOf(`!`);

  if (splitPoint !== -1) argv.splice(splitPoint, 1);

  let command;
  try {
    command = cli.process({
      input: argv,
      context: cli.defaultContext,
      partial: true,
    });
  } catch (error) {
    // Handle AmbiguousSyntaxError by falling back to lenient parsing
    if (error.constructor.name === `AmbiguousSyntaxError`)
      throw new Error(`Ambiguous command: ${line}`);

    throw error;
  }

  const tokens = command.tokens.flatMap(token => {
    if (token.segmentIndex < splitPoint) return [];
    if (token.type !== `option`) return [token];
    if (token.slice && token.slice[0] !== 0) return [token];

    const segment = argv[token.segmentIndex];
    const segmentLength = token.slice ? token.slice[1] : segment.length;
    const firstNonDashIndex = segment.search(/[^-]/);

    if (firstNonDashIndex === -1) return [token];

    return [
      {
        segmentIndex: token.segmentIndex,
        type: `dash`,
        slice: [0, firstNonDashIndex],
        option: token.option,
      },
      {
        segmentIndex: token.segmentIndex,
        type: `option`,
        slice: [firstNonDashIndex, segmentLength],
        option: token.option,
      },
    ];
  });

  const path = command.path;
  const definition = cli.definition(command.constructor);
  const tooltip = definition?.description
    ? capitalize(definition.description)
    : null;

  return {
    type: `command`,
    command: {name, path, argv},
    split: splitPoint !== -1,
    tooltip,
    tokens: tokens.map(token => ({
      ...token,
      text: getTokenText(token, argv),
      tooltip: getTokenTooltip(token, definition),
    })),
  };
}

function makeCommandOrRawLine(line, cli) {
  try {
    return makeCommandLine(line, cli);
  } catch {
    // Fallback to a lenient parser that tolerates placeholders, subshells, and inline comments
    try {
      return makeLenientCommandLine(line, cli);
    } catch {
      if (process.env.NODE_ENV === `development`)
        console.debug(`Failed to parse "${line}"`);

      return makeRawLine(line);
    }
  }
}

function makeLenientCommandLine(line, cli) {
  // Strip inline comments starting with a space+hash (to avoid heading lines already handled)
  const hashIndex = line.indexOf(` #`);
  const withoutComment = hashIndex !== -1 ? line.slice(0, hashIndex) : line;

  const trimmed = withoutComment.trim();
  if (trimmed.length === 0) return makeRawLine(line);

  const parts = trimmed.split(/\s+/);
  const [name, ...argv] = parts;

  if (name !== cli.binaryName)
    return makeRawLine(line);


  // Enhanced tokenization for lenient parsing
  const tokens = argv.map((arg, index) => {
    // Try to identify option tokens (starting with -)
    if (arg.startsWith(`-`)) {
      return {
        segmentIndex: index,
        type: `option`,
        text: arg,
        option: arg.startsWith(`--`) ? arg.slice(2) : arg.slice(1),
      };
    }

    // Try to identify path tokens (subcommands)
    if (index === 0) {
      return {
        segmentIndex: index,
        type: `path`,
        text: arg,
      };
    }

    // Default to text
    return {
      segmentIndex: index,
      type: `text`,
      text: arg,
    };
  });

  // Try to get a basic tooltip for the main command
  let tooltip = null;
  let path = [];

  if (argv.length > 0) {
    const subcommand = argv[0];
    path = [subcommand];

    // Try to find a definition for the subcommand
    try {
      const definitions = cli.definitions();
      const definition = definitions.find(
        def =>
          def.commandName === subcommand ||
          (def.path &&
            def.path.length > 0 &&
            def.path[def.path.length - 1] === subcommand),
      );

      if (definition?.description) {
        tooltip = definition.description;
      }
    } catch {
      // Ignore errors when trying to get tooltip
    }
  }

  return {
    type: `command`,
    command: {name, path, argv},
    split: false,
    tooltip,
    tokens,
  };
}

function renderCommandLine(line) {
  let firstPathToken = line.tokens.findIndex(token => token.type === `path`);
  if (firstPathToken === -1) firstPathToken = line.tokens.length;

  let firstNonPathToken = line.tokens.findIndex(
    (token, i) => i >= firstPathToken && token.type !== `path`,
  );
  if (firstNonPathToken === -1) firstNonPathToken = line.tokens.length;

  const renderTokens = (start, end) => {
    const tokens = line.tokens.slice(start, end);

    return tokens
      .map((token, index) => {
        const prevToken = tokens[index - 1] ?? line.tokens[start + index - 1];

        const needsSpace =
          index > 0 &&
          token.segmentIndex !== prevToken?.segmentIndex &&
          (token.type !== `path` || index > 0);

        const tooltip = token.tooltip
          ? escapeHtmlAttribute(token.tooltip)
          : null;

        const tokenContent = escapeHtml(token.text);

        const classes = [
          tooltip && `tooltip-link`,
          token.type === `option` && tooltip
            ? `underline decoration-dotted underline-offset-2 cursor-help`
            : null,
        ].filter(Boolean);

        const attributes = [
          `data-type="${token.type}"`,
          classes.length ? `class="${classes.join(` `)}"` : null,
          tooltip ? `aria-label="${tooltip}" data-tooltip="${tooltip}"` : null,
        ]
          .filter(Boolean)
          .join(` `);

        return `${
          needsSpace ? ` ` : ``
        }<span ${attributes}>${tokenContent}</span>`;
      })
      .join(``);
  };

  const wrapPath = content => {
    // Only create links for path tokens (subcommands) if there's a path and it's not an option (doesn't start with -)
    if (
      line.command.path &&
      line.command.path.length > 0 &&
      !line.command.path[0].startsWith(`-`)
    ) {
      const href = `/cli/${line.command.path.join(`/`)}`;
      const ariaLabel = line.tooltip
        ? escapeHtmlAttribute(line.tooltip)
        : `Create a new package`;

      return `<a href="${href}" class="tooltip-link" aria-label="${ariaLabel}">${content}</a>`;
    }
    return content;
  };

  const pathTokensContent = renderTokens(firstPathToken, firstNonPathToken);

  return (
    (!line.split
      ? `${escapeHtml(line.command.name)}${line.tokens.length > 0 ? ` ` : ``}`
      : ``) +
    (firstPathToken > 0 ? renderTokens(0, firstPathToken) : ``) +
    (firstPathToken > 0 && firstPathToken < line.tokens.length ? ` ` : ``) +
    (firstNonPathToken > 0 ? wrapPath(pathTokensContent) : ``) +
    (firstNonPathToken < line.tokens.length
      ? ` ${renderTokens(firstNonPathToken, line.tokens.length)}`
      : ``)
  );
}

function getTokenText(token, argv) {
  const arg = argv[token.segmentIndex];
  return token.slice ? arg.slice(token.slice[0], token.slice[1]) : arg;
}

function getTokenTooltip(token, definition) {
  if (!definition) return null;
  if (token.type !== `option`) return null;

  const option = definition.options.find(
    option => option.preferredName === token.option,
  );
  if (!option?.description) return null;

  return option.description;
}

function escapeHtml(text) {
  return text
    .replace(/&/g, `&amp;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`)
    .replace(/"/g, `&quot;`)
    .replace(/'/g, `&#039;`);
}

function escapeHtmlAttribute(value) {
  return String(value)
    .replace(/&/g, `&amp;`)
    .replace(/"/g, `&quot;`)
    .replace(/'/g, `&#39;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`);
}
