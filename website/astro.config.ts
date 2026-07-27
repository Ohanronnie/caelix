import { defineConfig } from "astro/config";
import { fileURLToPath } from "node:url";
import sitemap from "@astrojs/sitemap";
import { markdownLinks } from "./src/lib/markdown-links";

export default defineConfig({
  site: "https://ohanronnie.github.io/caelix/",
  base: "/caelix",
  outDir: fileURLToPath(new URL("../book", import.meta.url)),
  build: {
    format: "directory",
  },
  integrations: [sitemap()],
  markdown: {
    remarkPlugins: [markdownLinks],
    syntaxHighlight: "prism",
  },
});
