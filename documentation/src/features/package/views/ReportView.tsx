import {usePackageInfo, useReleaseReadme} from 'src/api/package';

interface ReportViewProps {
  name: string;
  version: string;
}

export default function ReportView({name, version}: ReportViewProps) {
  const pkgInfo = usePackageInfo(name);

  if (pkgInfo.error) return <div>{pkgInfo.error}</div>;

  const readmeContent = useReleaseReadme({
    name,
    readme: pkgInfo.readme,
    version,
    versions: pkgInfo.versions,
  });

  return (
    <div
      dangerouslySetInnerHTML={{
        __html: readmeContent as string,
      }}
    />
  );
}
