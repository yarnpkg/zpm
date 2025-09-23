import Arrow from "/src/assets/svg/right-arrow.svg?react";
import { formatPackageLink } from "@/utils/helpers";

interface PackageBreadcrumbsProps {
  name: string;
  version: string;
  file?: string;
}

export default function PackageBreadcrumbs({
  name,
  version,
  file,
}: PackageBreadcrumbsProps) {
  return (
    <div aria-label="Breadcrumb">
      <ul class="flex flex-wrap items-end gap-x-3 text-sm leading-5 font-montserrat">
        <li class="flex items-center gap-3">
          <a href="/" class="!no-underline hover:!underline !text-blue-50">
            Home
          </a>
          <span class="text-slate-400">
            <Arrow />
          </span>
        </li>
        <li class="flex items-center gap-3">
          <a
            href={formatPackageLink(name, version)}
            class="!no-underline hover:!underline !text-blue-50"
          >
            {name}
          </a>
          <span class="text-slate-400">{file && <Arrow />}</span>
        </li>
        {file && (
          <li class="flex items-center gap-3">
            <a
              href={formatPackageLink(name, version, file)}
              class="!no-underline hover:!underline !text-blue-50"
            >
              {file}
            </a>
          </li>
        )}
      </ul>
    </div>
  );
}
