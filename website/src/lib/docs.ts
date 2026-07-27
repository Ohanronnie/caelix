import fs from "node:fs";
import path from "node:path";
import type { CollectionEntry } from "astro:content";

export interface NavigationItem {
  title: string;
  id: string;
  children: NavigationItem[];
}

export interface NavigationSection {
  title: string;
  items: NavigationItem[];
}

export const docsRoot = path.resolve(process.cwd(), "../docs/src");

export function getDocsUrl(id: string): string {
  return getBaseUrl(`${id.replace(/^\/+|\/+$/g, "")}/`);
}

export function getBaseUrl(pathname = ""): string {
  const base = import.meta.env.BASE_URL.endsWith("/")
    ? import.meta.env.BASE_URL
    : `${import.meta.env.BASE_URL}/`;
  return `${base}${pathname.replace(/^\//, "")}`;
}

export function getNavigation(): NavigationSection[] {
  const summary = fs.readFileSync(path.join(docsRoot, "SUMMARY.md"), "utf8");
  const sections: NavigationSection[] = [];
  let section: NavigationSection | undefined;
  let parents: Array<{ depth: number; item: NavigationItem }> = [];

  for (const line of summary.split("\n")) {
    const heading = line.match(/^# (?!Summary$)(.+)$/);
    if (heading) {
      section = { title: heading[1], items: [] };
      sections.push(section);
      parents = [];
      continue;
    }

    const item = line.match(/^(\s*)-?\s*\[([^\]]+)\]\(([^)]+\.md)\)$/);
    if (!item || !section) continue;

    const navigationItem: NavigationItem = {
      title: item[2],
      id: item[3].replace(/^\.\//, "").replace(/\.md$/, ""),
      children: [],
    };
    const depth = Math.floor(item[1].length / 2);

    while (parents.length && parents.at(-1)!.depth >= depth) parents.pop();
    const parent = parents.at(-1)?.item;
    (parent ? parent.children : section.items).push(navigationItem);
    parents.push({ depth, item: navigationItem });
  }

  return sections;
}

export function flattenNavigation(
  sections = getNavigation(),
): NavigationItem[] {
  return sections.flatMap((section) => flattenItems(section.items));
}

function flattenItems(items: NavigationItem[]): NavigationItem[] {
  return items.flatMap((item) => [item, ...flattenItems(item.children)]);
}

export function getPageTitle(entry: CollectionEntry<"docs">): string {
  const content = fs.readFileSync(entry.filePath!, "utf8");
  return content.match(/^#\s+(.+)$/m)?.[1]?.replace(/`/g, "") ?? entry.id;
}
