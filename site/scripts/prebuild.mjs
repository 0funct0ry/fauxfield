// Runs before `astro build` and `astro dev` (via `pnpm prebuild`/Makefile):
//  1. copies CHANGELOG.md into the docs as /docs/changelog/;
//  2. reads the latest GitHub release into src/data/release.json (download links, version banner).
// Offline or before the first release it falls back to the Releases page.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repo = "0funct0ry/fauxfield";

const log = readFileSync(resolve(root, "../CHANGELOG.md"), "utf8").replace(/^# Changelog\s*/, "");
writeFileSync(
  resolve(root, "src/content/docs/docs/changelog.md"),
  `---\ntitle: Changelog\ndescription: What changed in each Fauxfield release.\n---\n\n<!-- Copied from CHANGELOG.md by site/scripts/prebuild.mjs; do not edit. -->\n\n${log.replace(/^## /gm, "## ").replace(/^### /gm, "### ")}`,
);

let release = { tag: null, url: `https://github.com/${repo}/releases`, assets: [] };
try {
  const headers = { accept: "application/vnd.github+json" };
  if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  const res = await fetch(`https://api.github.com/repos/${repo}/releases/latest`, { headers, signal: AbortSignal.timeout(8000) });
  if (res.ok) {
    const j = await res.json();
    release = {
      tag: j.tag_name,
      url: j.html_url,
      assets: (j.assets ?? []).map((a) => ({ name: a.name, url: a.browser_download_url })),
    };
  }
} catch {
  /* offline: keep the fallback */
}
mkdirSync(resolve(root, "src/data"), { recursive: true });
writeFileSync(resolve(root, "src/data/release.json"), JSON.stringify(release, null, 2) + "\n");
console.log(`site prebuild: release ${release.tag ?? "none yet"}`);
