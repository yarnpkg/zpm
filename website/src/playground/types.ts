export type PlaygroundEntry = {
  content?: string;
  depth: number;
  kind: `file` | `folder` | `terminal`;
  language?: string;
  name: string;
  path: string;
};

export type PlaygroundTemplate = {
  description: string;
  entries: Array<PlaygroundEntry>;
  id: string;
  label: string;
};

export type PlaygroundTemplateManifest = {
  description?: string;
  label: string;
  order?: number;
};
