import type {BaseData}  from '@clipanion/astro';
import type {Component} from '@clipanion/tools';

type DataEntry = {id: string, data: BaseData, filePath?: string};

type OptionComponent = Extract<Component, {type: `option`}>;
type PositionalComponent = Extract<Component, {type: `positional`}>;

type Example = {description: string, command: string};

const EXAMPLES_HEADING = `## Examples`;
const EXAMPLE_COMMAND_REGEXP = /^`([^`]+)`$/;

// clipanion-rs doesn't populate `commandSpec.examples` from the command
// definitions yet, so the commands document their examples in a final
// `## Examples` section of their details, as alternating description and
// `` `command` `` paragraphs (see contributing/design/cli-commands.md). We
// extract this section so it can be rendered after the options, the same
// way as the examples coming from the command spec.
function extractExamples(entry: DataEntry, details: string): {details: string, examples: Array<Example>} {
  const paragraphs = details.split(/\n\n/);

  const headingIndex = paragraphs.findIndex(paragraph => paragraph.trim() === EXAMPLES_HEADING);
  if (headingIndex === -1)
    return {details, examples: []};

  const commandName = [entry.data.binaryName, ...entry.data.commandSpec.primaryPath].join(` `);
  const examples: Array<Example> = [];

  let description: string | null = null;
  for (const paragraph of paragraphs.slice(headingIndex + 1).map(paragraph => paragraph.trim())) {
    const match = paragraph.match(EXAMPLE_COMMAND_REGEXP);
    if (match) {
      if (description === null)
        throw new Error(`The examples section of \`${commandName}\` has a command without description: ${paragraph}`);

      examples.push({description, command: match[1]});
      description = null;
    } else {
      if (description !== null)
        throw new Error(`The examples section of \`${commandName}\` has a description without command: ${description}`);

      description = paragraph;
    }
  }

  if (description !== null)
    throw new Error(`The examples section of \`${commandName}\` has a description without command: ${description}`);

  return {
    details: paragraphs.slice(0, headingIndex).join(`\n\n`).trim(),
    examples,
  };
}

function escapeDirective(s: string): string {
  return s.replace(/\\/g, `\\\\`).replace(/\[/g, `\\[`).replace(/\]/g, `\\]`);
}

function formatOptionNames(option: OptionComponent): string {
  const names = [option.primaryName, ...option.aliases];
  return names.join(`, `);
}

function inferOptionType(option: OptionComponent): string {
  if (!option.allowBinding && !option.allowBoolean)
    return `boolean`;

  return `string`;
}

function buildUsageLine(entry: DataEntry): string {
  const {binaryName, commandSpec} = entry.data;
  const parts = [binaryName, ...commandSpec.primaryPath];

  for (const component of commandSpec.components) {
    if (component.type !== `positional`) continue;
    const pos = component as PositionalComponent;
    if (pos.positionalType === `keyword`) {
      parts.push(pos.expected!);
    } else {
      const name = pos.name || `arg`;
      const suffix = pos.extra_len !== 0 ? `…` : ``;
      parts.push(`<${name}${suffix}>`);
    }
  }

  return parts.join(` `);
}

export function cliBody(entry: DataEntry): string {
  const {commandSpec} = entry.data;
  const lines: Array<string> = [];

  lines.push(`\`\`\`terminal`);
  lines.push(buildUsageLine(entry));
  lines.push(`\`\`\``);

  const {details, examples: detailsExamples} = extractExamples(entry, commandSpec.documentation?.details ?? ``);
  const examples = [...commandSpec.examples, ...detailsExamples];

  if (details) {
    lines.push(``);
    lines.push(details);
  }

  const options = commandSpec.components.filter(
    (c): c is OptionComponent => c.type === `option` && !(c as OptionComponent).isHidden,
  ) as Array<OptionComponent>;

  if (options.length > 0) {
    for (const option of options) {
      const names = formatOptionNames(option);
      const type = inferOptionType(option);
      const pills = `:type[${escapeDirective(type)}]`;

      lines.push(``);
      lines.push(`### \`${names}\` ${pills}`);

      // clipanion-rs reads the first paragraph of an option's doc comment as
      // its description, and puts the other paragraphs in its details
      const description = [option.documentation?.description, option.documentation?.details]
        .filter(Boolean)
        .join(`\n\n`);

      if (description) {
        lines.push(``, description);
      }
    }
  }

  if (examples.length > 0) {
    lines.push(``);
    lines.push(`## Examples`);

    for (const example of examples) {
      lines.push(``);
      if (example.description)
        lines.push(`**${example.description}**`);

      lines.push(``);
      lines.push(`\`\`\`terminal`);
      lines.push(example.command);
      lines.push(`\`\`\``);
    }
  }

  return lines.join(`\n`);
}
