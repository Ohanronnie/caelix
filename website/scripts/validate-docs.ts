import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const docsRoot = fileURLToPath(new URL("../../docs/src", import.meta.url));
const markdownFiles = walk(docsRoot).filter(
  (file) => file.endsWith(".md") && !file.endsWith("SUMMARY.md"),
);
const errors: string[] = [];
const summary = fs.readFileSync(path.join(docsRoot, "SUMMARY.md"), "utf8");
const summaryPaths = [...summary.matchAll(/\[[^\]]+\]\(([^)]+\.md)\)/g)].map(
  (match) => match[1].replace(/^\.\//, ""),
);

for (const file of markdownFiles) {
  const relative = path.relative(docsRoot, file).replace(/\\/g, "/");
  if (!summaryPaths.includes(relative))
    errors.push(`${relative} is missing from SUMMARY.md`);

  const content = fs.readFileSync(file, "utf8");
  const links = [...content.matchAll(/\[[^\]]*\]\(([^)]+)\)/g)].map(
    (match) => match[1],
  );
  for (const href of links) validateInternalLink(relative, href);
}

for (const summaryPath of summaryPaths) {
  if (!fs.existsSync(path.join(docsRoot, summaryPath)))
    errors.push(`SUMMARY.md references missing file ${summaryPath}`);
}

if (errors.length) {
  console.error(
    `Documentation validation failed:\n${errors.map((error) => `- ${error}`).join("\n")}`,
  );
  process.exit(1);
}

console.log(
  `Validated ${markdownFiles.length} documentation pages, SUMMARY.md routes, internal links, and heading anchors.`,
);

function validateInternalLink(source: string, href: string): void {
  if (
    href.startsWith("#") ||
    href.startsWith("/") ||
    /^[a-z][a-z\d+.-]*:/i.test(href)
  )
    return;
  const match = href.match(/^([^?#]*\.md)(?:\?[^#]*)?(#.+)?$/i);
  if (!match) return;

  const target = path.normalize(path.join(path.dirname(source), match[1]));
  const targetPath = path.join(docsRoot, target);
  if (!targetPath.startsWith(docsRoot) || !fs.existsSync(targetPath)) {
    errors.push(`${source} links to missing Markdown file ${href}`);
    return;
  }

  if (
    match[2] &&
    !headingSlugs(fs.readFileSync(targetPath, "utf8")).has(match[2].slice(1))
  ) {
    errors.push(`${source} links to missing heading ${href}`);
  }
}

function headingSlugs(content: string): Set<string> {
  const seen = new Map<string, number>();
  const slugs = new Set<string>();
  for (const heading of content.matchAll(/^#{1,6}\s+(.+)$/gm)) {
    const base = heading[1]
      .replace(/`/g, "")
      .trim()
      .toLowerCase()
      .replace(/[^\w\s-]/g, "")
      .replace(/\s+/g, "-")
      .replace(/-+/g, "-");
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    slugs.add(count ? `${base}-${count}` : base);
  }
  return slugs;
}

function walk(directory: string): string[] {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const target = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(target) : [target];
  });
}
