import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { fileURLToPath } from "node:url";

const docs = defineCollection({
  loader: glob({
    base: fileURLToPath(new URL("../../docs/src", import.meta.url)),
    pattern: "**/*.md",
  }),
});

export const collections = { docs };
