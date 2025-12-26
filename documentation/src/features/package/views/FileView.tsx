import {useReleaseFile} from '@/api/package';
import {Editor}         from '@monaco-editor/react';
import {Suspense}       from 'preact/compat';

interface FileViewProps {
  name: string;
  version: string;
  path: string;
}

function FileView({name, version, path}: FileViewProps) {
  const fileContent = useReleaseFile({name, version, path});

  return (
    <div className={`w-full h-auto flex flex-col gap-y-2`}>
      <div className={`leading-none tracking-[6%] py-2 max-w-3xl break-words bg-gradient-to-b from-[#656E98] to-white to-60% text-transparent bg-clip-text text-3xl`}>
        {path}
      </div>
      <Editor
        path={location.href}
        value={fileContent}
        theme={`vs-dark`}
        options={{
          readOnly: true,
          scrollBeyondLastLine: false,
          automaticLayout: true,
        }}
        defaultLanguage={`javascript`}
        height={`90vh`}
        loading={<EditorFallback />}
      />
    </div>
  );
}

export default function SuspenseRenderer(props: FileViewProps) {
  return (
    <Suspense fallback={<EditorFallback />}>
      <FileView {...props} />
    </Suspense>
  );
}

const EditorFallback = () => (
  <div className={`flex items-center justify-center min-h-[400px] p-4 w-full`}>
    <div className={`animate-pulse bg-gray-100/5 rounded-lg h-64 w-full`} />
  </div>
);
