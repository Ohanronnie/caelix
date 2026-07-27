import path from "node:path";
import type { Plugin } from "unified";
import type { Root } from "mdast";

const docsRoot = path.resolve(process.cwd(), "../docs/src");
const base = "/caelix";

/** Rewrites authored relative Markdown links to their generated clean URLs. */
export const markdownLinks: Plugin<[], Root> = () => {
  return (tree, file) => {
    const visit = (node: any): void => {
      if (node.type === "link" && typeof node.url === "string") {
        node.url = rewriteLink(node.url, file.path);
      }

      if (Array.isArray(node.children)) {
        node.children.forEach(visit);
      }
    };

    visit(tree);
  };
};

function rewriteLink(url: string, sourcePath?: string): string {
  if (
    !sourcePath ||
    url.startsWith("#") ||
    url.startsWith("/") ||
    /^[a-z][a-z\d+.-]*:/i.test(url)
  ) {
    return url;
  }

  const match = url.match(/^([^?#]*\.md)([?#].*)?$/i);
  if (!match) return url;

  const target = path.resolve(path.dirname(sourcePath), match[1]);
  const relativeTarget = path.relative(docsRoot, target);
  if (relativeTarget.startsWith("..") || path.isAbsolute(relativeTarget))
    return url;

  return `${base}/${relativeTarget.replace(/\\/g, "/").replace(/\.md$/i, "")}/${match[2] ?? ""}`;
}
