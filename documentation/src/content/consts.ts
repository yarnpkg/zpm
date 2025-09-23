export const NAVIGATION = [
  { href: "/getting-started", title: "Get Started" },
  { href: "/features/caching", title: "Features" },
  { href: "/cli", title: "CLI" },
  { href: "/configuration/manifest", title: "Configuration" },
  { href: "/advanced/error-codes", title: "Advanced" },
  { href: "/blog/", title: "Blog" },
] as const;

export let breadcrumbsHolder: { breadcrumbs?: object } = {
  breadcrumbs: undefined,
};
