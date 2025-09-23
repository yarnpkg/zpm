export type CommandPart = {
  type: "text" | "link";
  value: string;
  href?: string;
};

export type CommandLine = {
  parts: CommandPart[];
};

export type CommandSlideProps = {
  command: string | CommandLine[];
  title?: string | null;
  name?: string;
  description?: string | null;
  link?: string;
};
