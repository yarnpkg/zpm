// import cn from "@/utils/cn";
import { useReleaseInfo } from "src/api/package";
import Collapsable from "./Collapsable";
import { FileTreeRenderer, FileTree } from "./FileTree";

interface FilesProps {
  name: string;
  version: string;
}

export default function Files({ name, version }: FilesProps) {
  const releaseData = useReleaseInfo({ name, version });

  if (!releaseData?.files) return null;

  const fileTree = new FileTree(
    releaseData.files.map((file) => {
      let path = file.name.split("/").filter(Boolean);
      path = [file.name, ...path];
      return path;
    })
  );

  return (
    <Collapsable label="Files" id="file-list" expandFull>
      <FileTreeRenderer
        node={fileTree.tree}
        basePath="/"
        name={name}
        version={version}
        currentPath={location.pathname}
      />
    </Collapsable>
  );
}
