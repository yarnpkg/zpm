import {formatPackageLink} from '@/utils/helpers';
import gitUrlParse         from 'git-url-parse';
import {usePackageInfo}    from 'src/api/package';
import GithubIcon          from 'src/assets/svg/github-icon.svg?react';
import InformationIcon     from 'src/assets/svg/information-icon.svg?react';
import RunkitIcon          from 'src/assets/svg/runkit-icon.svg?react';
import WebsiteIcon         from 'src/assets/svg/website-icon.svg?react';

interface PackageEntryProps {
  name: string;
  version: string;
}

export default function PackageEntry({name, version}: PackageEntryProps) {
  const packageData = usePackageInfo(name);

  if (packageData.error) return null;

  const selectedVersionData = packageData.versions[version];

  const homepageLink = selectedVersionData.homepage;
  const repository = selectedVersionData.repository;

  let githubRepoUrl: string | undefined;

  if (repository?.url) {
    const parsedRepoUrl = gitUrlParse(repository.url);

    if (
      parsedRepoUrl.source === `github.com` &&
      parsedRepoUrl.owner &&
      parsedRepoUrl.name
    ) {
      githubRepoUrl = `https://github.com/${parsedRepoUrl.owner}/${parsedRepoUrl.name}`;
      if (repository.directory) {
        githubRepoUrl += `/tree/HEAD/${repository.directory}`;
      }
    }
  }

  return (
    <ul className={`flex flex-col gap-y-6 text-blue-300 px-8`}>
      <li>
        <button
          onClick={() => {
            window.history.pushState(
              {},
              ``,
              formatPackageLink(packageData.name, version),
            );
            window.dispatchEvent(new PopStateEvent(`popstate`));
          }}
          class={`flex items-center gap-x-2`}
        >
          <InformationIcon class={`size-6 stroke-current`} />
          <span class={`font-medium text-white hover:text-blue-50 transition-colors leading-[1.4em]`}>
            Information
          </span>
        </button>
      </li>
      {typeof homepageLink === `string` && (
        <li>
          <a
            href={homepageLink}
            aria-label={`Website`}
            target={`_blank`}
            rel={`noopener noreferrer`}
            class={`flex items-center gap-x-2`}
          >
            <WebsiteIcon class={`size-6 stroke-current`} />
            <span class={`font-medium text-white hover:text-blue-50 transition-colors leading-[1.4em]`}>
              Website
            </span>
          </a>
        </li>
      )}
      {typeof githubRepoUrl === `string` && (
        <li>
          <a
            href={githubRepoUrl}
            aria-label={`Repository`}
            target={`_blank`}
            rel={`noopener noreferrer`}
            class={`flex items-center gap-x-2`}
          >
            <GithubIcon class={`size-6 stroke-current`} />
            <span class={`font-medium text-white hover:text-blue-50 transition-colors leading-[1.4em]`}>
              Repository
            </span>
          </a>
        </li>
      )}
      <li>
        <a
          href={`https://npm.runkit.com/${packageData.name}`}
          target={`_blank`}
          rel={`noopener noreferrer`}
          aria-label={`Runkit`}
          class={`flex items-center gap-x-2`}
        >
          <RunkitIcon class={`size-6 stroke-current`} />
          <span class={`font-medium text-white hover:text-blue-50 transition-colors leading-[1.4em]`}>
            Runkit
          </span>
        </a>
      </li>
    </ul>
  );
}
