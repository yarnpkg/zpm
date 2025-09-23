import { capitalize } from "es-toolkit/compat";
import { dedent, generateFrontmatter, toHyphenCase } from "./helpers";

interface Command {
  path: string;
  description: string;
}

export function createIndexContent(
  packageName: string,
  shortName: string,
  commands: Command[]
): string {
  const isCorePackage = packageName === "@yarnpkg/cli";

  return dedent(`
      ${generateFrontmatter({
        title: packageName,
        description: `Documentation for all ${shortName} package commands`,
        slug: shortName === "cli" ? "cli" : `cli/${shortName}`,
        sidebar_position: 1,
        category: shortName === "cli" ? "cli" : `cli/${shortName}`,
      })}
      
      import { Card, CardGrid, LinkCard } from "@astrojs/starlight/components";
          
      ${
        !isCorePackage
          ? dedent(`
            :::note
            To use these commands, you need to use the [\`${packageName}\`](https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-${shortName}/README.md) package either:
            - By installing it locally using [\`yarn add\`](/cli/add) and running it using [\`yarn run\`](/cli/run)
            - By downloading and running it in a temporary environment using [\`yarn dlx\`](/cli/dlx)
            :::
          `)
          : ""
      }
      
        ${commands
          .map(
            ({ path, description }: { path: string; description: string }) => {
              const urlPath = path.split(" ").slice(1).join("/");

              return dedent(`
              <LinkCard 
                title="${path}" 
                href="${
                  shortName === "cli" ? "/cli" : `/cli/${shortName}`
                }/${toHyphenCase(urlPath)}" 
                description="${capitalize(description)}"
              />
            `);
            }
          )
          .join("\n")}
    `);
}

export function createCommandContent(
  packageName: string,
  shortName: string,
  commandInfo: any
): string {
  const isCorePackage = packageName === "@yarnpkg/cli";
  const commandSlug = commandInfo.path.split(" ").slice(1).join("/");

  const frontMatter = {
    title: commandInfo.path,
    description: commandInfo.description || "Yarn CLI command",
    slug:
      shortName === "cli"
        ? `cli/${toHyphenCase(commandSlug)}`
        : `cli/${shortName}/${toHyphenCase(commandSlug)}`,
    category: shortName === "cli" ? "cli" : `cli/${shortName}`,
  };

  const sections = [
    generateFrontmatter(frontMatter),
    dedent(`import { Code } from "@astrojs/starlight/components";`),

    // Package Note
    !isCorePackage
      ? dedent(`
          :::note
          To use this command, you need to use the [\`${packageName}\`](https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-${shortName}/README.md) package either:
          - By installing it locally using [\`yarn add\`](/cli/add) and running it using [\`yarn run\`](/cli/run)
          - By downloading and running it in a temporary environment using [\`yarn dlx\`](/cli/dlx)
          :::
        `)
      : null,

    // Description
    frontMatter.description
      ? dedent(`
          <div className="subtitle">
          ${capitalize(frontMatter.description)}
          </div>
        `)
      : null,

    // Usage
    commandInfo.usage
      ? dedent(`
          ## Usage
          <Code code="${commandInfo.usage.replace(
            /"/g,
            "&quot;"
          )}" lang="bash" />
        `)
      : null,

    // Examples
    commandInfo.examples?.length
      ? dedent(`
          ## Examples
          ${commandInfo.examples
            .map(([description, example]: string[]) =>
              dedent(`
              <p>${description}:</p>
              <Code code="${example.replace(/"/g, "&quot;")}" lang="bash" />
            `)
            )
            .join("\n\n")}
        `)
      : null,

    // Details
    commandInfo.details
      ? dedent(`
          ## Details
          ${commandInfo.details}
        `)
      : null,

    // Options
    commandInfo.options?.length
      ? dedent(`
          ## Options
          | Definition | Description |
          | ---------- | ----------- |
          ${commandInfo.options
            .map(
              ({
                definition: optDef,
                description,
              }: {
                definition: string;
                description: string;
              }) =>
                dedent(`
              | <h4 id="${encodeURIComponent(
                `options-${optDef}`.replace(/-+/g, "-")
              )}" className="header-code"><code className="language-text">${optDef}</code></h4> | ${description} |
            `)
            )
            .join("\n")}
        `)
      : null,
  ];

  return sections.filter(Boolean).join("\n\n");
}
