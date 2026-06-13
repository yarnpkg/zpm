import type {BaseData}  from '@clipanion/astro';
import type {Component} from '@clipanion/tools';

type DataEntry = {id: string, data: BaseData, filePath?: string};

type OptionComponent = Extract<Component, {type: `option`}>;
type PositionalComponent = Extract<Component, {type: `positional`}>;

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

  if (commandSpec.documentation?.details) {
    lines.push(``);
    lines.push(commandSpec.documentation.details);
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

      if (option.documentation?.description) {
        lines.push(``, option.documentation.description);
      }
    }
  }

  if (commandSpec.examples.length > 0) {
    lines.push(``);
    lines.push(`## Examples`);

    for (const example of commandSpec.examples) {
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
