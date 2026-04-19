import Markdown, {type Components} from 'react-markdown';
import {useEffect, useState}       from 'react';
import rehypeGithubAlert           from 'rehype-github-alert';
import rehypeGithubColor           from 'rehype-github-color';
import rehypeGithubDir             from 'rehype-github-dir';
import rehypeGithubEmoji           from 'rehype-github-emoji';
import rehypeGithubHeading         from 'rehype-github-heading';
import rehypeGithubImage           from 'rehype-github-image';
import rehypeRaw                   from 'rehype-raw';
import remarkGfm                   from 'remark-gfm';

import {useDaemon}                 from '../lib/daemon-context';

const MIME_TYPES: Record<string, string> = {
  png: `image/png`,
  jpg: `image/jpeg`,
  jpeg: `image/jpeg`,
  gif: `image/gif`,
  webp: `image/webp`,
  svg: `image/svg+xml`,
  bmp: `image/bmp`,
  ico: `image/x-icon`,
};

function getMimeType(src: string): string {
  const ext = src.split(`.`).pop()?.toLowerCase() ?? ``;
  return MIME_TYPES[ext] ?? `application/octet-stream`;
}

function isRelativeUrl(src: string): boolean {
  return !src.startsWith(`http://`) && !src.startsWith(`https://`) && !src.startsWith(`data:`);
}

function ProjectImage(props: React.ImgHTMLAttributes<HTMLImageElement>) {
  const daemon = useDaemon();
  const [dataUri, setDataUri] = useState<string | null>(null);

  const src = props.src ?? ``;
  const isRelative = isRelativeUrl(src);

  useEffect(() => {
    if (!daemon || !isRelative || !src)
      return undefined;

    let cancelled = false;
    let blobUrl: string | null = null;

    daemon.readFile(src).then(result => {
      if (cancelled || !result)
        return;

      if (result.encoding === `base64`) {
        setDataUri(`data:${getMimeType(src)};base64,${result.content}`);
      } else {
        const blob = new Blob([result.content], {type: getMimeType(src)});
        blobUrl = URL.createObjectURL(blob);
        setDataUri(blobUrl);
      }
    }).catch(() => {
      // Image not available
    });

    return () => {
      cancelled = true;
      if (blobUrl)
        URL.revokeObjectURL(blobUrl);
    };
  }, [daemon, src, isRelative]);

  if (!isRelative)
    return <img {...props} />;

  if (!dataUri)
    return null;

  return <img {...props} src={dataUri} />;
}

const markdownComponents: Components = {
  img: ProjectImage,
};

const remarkPlugins = [remarkGfm];
const rehypePlugins = [
  rehypeRaw,
  rehypeGithubAlert,
  rehypeGithubColor,
  rehypeGithubDir,
  rehypeGithubEmoji,
  rehypeGithubHeading,
  rehypeGithubImage,
];

export function GithubMarkdown({content, className}: {content: string, className?: string}) {
  return (
    <article className={`markdown-body${className ? ` ${className}` : ``}`}>
      <Markdown remarkPlugins={remarkPlugins} rehypePlugins={rehypePlugins} components={markdownComponents}>
        {content}
      </Markdown>
    </article>
  );
}
