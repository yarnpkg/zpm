export type CommandPart = {
  type: `text` | `link`;
  value: string;
  href?: string;
};

export type CommandLine = {
  parts: Array<CommandPart>;
};

export type CommandSlideProps = {
  command: string | Array<CommandLine>;
  title?: string | null;
  name?: string;
  description?: string | null;
  link?: string;
};
