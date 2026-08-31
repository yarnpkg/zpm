type WorkspaceOptions = {
  cwd: string;
  packageManager: string;
};

export function createWorkspace(options: WorkspaceOptions) {
  return {
    async install() {
      return {
        cwd: options.cwd,
        resolved: 42,
        linked: true,
      };
    },

    async explain(ident: string) {
      return `${ident} is provided by workspace:demo`;
    },
  };
}
