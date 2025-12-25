export const NAVIGATION = [
  {href: `/getting-started`, title: `Get Started`},
  {href: `/concepts/workspaces`, title: `Concepts`},
  {href: `/configuration/manifest`, title: `Reference`},
  {href: `/appendix/workspaces-and-peer-deps`, title: `Appendix`},
  {href: `/contributing/welcome`, title: `Contributing`},
  {href: `/blog`, title: `Blog`},
] as const;

export const breadcrumbsHolder: {breadcrumbs?: object} = {
  breadcrumbs: undefined,
};
