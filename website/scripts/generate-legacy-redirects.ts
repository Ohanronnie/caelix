import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const docsRoot = fileURLToPath(new URL("../../docs/src", import.meta.url));
const outputRoot = fileURLToPath(new URL("../../book", import.meta.url));
const base = "/caelix";
const pages = walk(docsRoot).filter(
  (file) => file.endsWith(".md") && !file.endsWith("SUMMARY.md"),
);

for (const file of pages) {
  const id = path
    .relative(docsRoot, file)
    .replace(/\\/g, "/")
    .replace(/\.md$/, "");
  const destination = `${base}/${id}/`;
  const output = path.join(outputRoot, `${id}.html`);
  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.writeFileSync(output, redirectPage(destination));
}

console.log(`Generated ${pages.length} legacy .html redirect pages.`);

function redirectPage(destination: string): string {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=${destination}"><link rel="canonical" href="${destination}"><title>Moved · Caelix</title><script>location.replace(${JSON.stringify(destination)})</script></head><body><p>This documentation page moved to <a href="${destination}">${destination}</a>.</p></body></html>`;
}

function walk(directory: string): string[] {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const target = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(target) : [target];
  });
}
