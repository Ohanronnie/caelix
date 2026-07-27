import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const docsRoot = fileURLToPath(new URL("../../docs/src", import.meta.url));
const outputRoot = fileURLToPath(new URL("../../book", import.meta.url));
const pages = walk(docsRoot).filter(
  (file) => file.endsWith(".md") && !file.endsWith("SUMMARY.md"),
);
const errors: string[] = [];

for (const file of pages) {
  const id = path
    .relative(docsRoot, file)
    .replace(/\\/g, "/")
    .replace(/\.md$/, "");
  const page = path.join(outputRoot, id, "index.html");
  const redirect = path.join(outputRoot, `${id}.html`);
  if (!fs.existsSync(page))
    errors.push(`Missing generated page: ${path.relative(outputRoot, page)}`);
  if (!fs.existsSync(redirect))
    errors.push(
      `Missing legacy redirect: ${path.relative(outputRoot, redirect)}`,
    );
}

if (!fs.existsSync(path.join(outputRoot, "pagefind", "pagefind.js")))
  errors.push("Missing Pagefind index.");
if (errors.length) {
  console.error(
    `Build output validation failed:\n${errors.map((error) => `- ${error}`).join("\n")}`,
  );
  process.exit(1);
}

console.log(
  `Validated ${pages.length} clean documentation routes, legacy redirects, and the Pagefind index.`,
);

function walk(directory: string): string[] {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const target = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(target) : [target];
  });
}
