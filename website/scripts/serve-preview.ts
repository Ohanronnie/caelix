import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const base = "/caelix";
const outputRoot = fileURLToPath(new URL("../../book", import.meta.url));
const port = Number(process.env.PORT ?? 4321);
const mimeTypes: Record<string, string> = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".wasm": "application/wasm",
};

Bun.serve({
  port,
  fetch(request) {
    const url = new URL(request.url);
    if (url.pathname !== base && !url.pathname.startsWith(`${base}/`))
      return new Response("Not found", { status: 404 });
    const relative = decodeURIComponent(
      url.pathname.slice(base.length),
    ).replace(/^\/+/, "");
    const candidate =
      relative.endsWith("/") || !path.extname(relative)
        ? path.join(outputRoot, relative, "index.html")
        : path.join(outputRoot, relative);
    const resolved = path.resolve(candidate);
    if (!resolved.startsWith(outputRoot) || !fs.existsSync(resolved))
      return new Response("Not found", { status: 404 });
    return new Response(Bun.file(resolved), {
      headers: {
        "content-type":
          mimeTypes[path.extname(resolved)] ?? "application/octet-stream",
      },
    });
  },
});

console.log(`Previewing ${outputRoot} at http://127.0.0.1:${port}${base}/`);
