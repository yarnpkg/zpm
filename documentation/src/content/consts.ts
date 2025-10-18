export const NAVIGATION = [
  {href: `/getting-started`, title: `Get Started`},
  {href: `/concepts/workspaces`, title: `Concepts`},
  {href: `/cli`, title: `CLI`},
  {href: `/configuration/manifest`, title: `Configuration`},
  {href: `/advanced/error-codes`, title: `Advanced`},
  {href: `/blog/`, title: `Blog`},
] as const;

export const breadcrumbsHolder: {breadcrumbs?: object} = {
  breadcrumbs: undefined,
};
