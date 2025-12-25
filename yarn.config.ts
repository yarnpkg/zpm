import {type Yarn} from "@yarnpkg/types";

export default {
  constraints: async ({Yarn}) => {
    for (const workspace of Yarn.workspaces()) {
      workspace.set(`repository.type`, `git`);
      workspace.set(`repository.url`, `git+https://github.com/yarnpkg/zpm.git`);
      workspace.set(`repository.directory`, workspace.cwd);
    }
  },
} satisfies Yarn.Config;
