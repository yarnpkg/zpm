import type { SidebarEntry } from "node_modules/@astrojs/starlight/utils/routing/types";
import type { HTMLAttributes } from "preact/compat";

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: "tip" | "note" | "success" | "danger" | "caution" | "package" | "author" | "default";
  text: string;
  className?: string;
}

export interface SidebarLinkProps extends HTMLAttributes<HTMLAnchorElement> {
  badge: BadgeProps | undefined;
  label: string;
  isCurrent: boolean;
  attrs?: HTMLAttributes<HTMLAnchorElement>;
  className?: string;
  variant: "link" | "sub-link";
  type: "link";
  initialCollapsed?: boolean; // SidebarGroup requires it
}

export interface SidebarGroupProps extends HTMLAttributes<HTMLDivElement> {
  badge: BadgeProps | undefined;
  label: string;
  collapsed: boolean;
  initialCollapsed: boolean;
  entries: SidebarEntry[];
  className?: string;
  type: "group";
  variant?: string;
}

export type SidebarEntryProps = SidebarLinkProps | SidebarGroupProps;
