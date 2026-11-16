import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import { unified } from "@astrojs/markdown-remark";

// Deployed to GitHub Pages at https://0funct0ry.github.io/fauxfield/. Override for a custom domain.
const site = process.env.SITE_URL ?? "https://0funct0ry.github.io";
const base = process.env.SITE_BASE ?? "/fauxfield";

/** Markdown links are written as `/docs/...`; prefix the deploy base so they work on Pages. */
function rehypeBase() {
  const walk = (n) => {
    if (n.type === "element" && n.tagName === "a" && typeof n.properties?.href === "string") {
      const h = n.properties.href;
      if (base && h.startsWith("/") && !h.startsWith("//") && !h.startsWith(base + "/")) {
        n.properties.href = base + h;
      }
    }
    n.children?.forEach(walk);
  };
  return (tree) => walk(tree);
}

export default defineConfig({
  site,
  base,
  trailingSlash: "always",
  markdown: unified({ rehypePlugins: [rehypeBase] }),
  integrations: [
    starlight({
      title: "Fauxfield",
      description: "Docs for Fauxfield, the local-first mock API tool.",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/0funct0ry/fauxfield" }],
      editLink: { baseUrl: "https://github.com/0funct0ry/fauxfield/edit/main/site/" },
      customCss: [
        "@fontsource/ibm-plex-sans/400.css",
        "@fontsource/ibm-plex-sans/600.css",
        "@fontsource/ibm-plex-mono/400.css",
        "./src/styles/theme.css",
      ],
      routeMiddleware: "./src/routeData.ts",
      head: [{ tag: "meta", attrs: { property: "og:type", content: "website" } }],
      sidebar: [
        { label: "Getting started", items: [{ autogenerate: { directory: "docs/getting-started" } }] },
        {
          label: "Guides",
          items: [
            "docs/guides/workspaces", "docs/guides/running-servers", "docs/guides/routes",
            "docs/guides/templating", "docs/guides/editor", "docs/guides/datasets-and-crud",
            "docs/guides/network-and-errors", "docs/guides/proxy-and-recording",
            "docs/guides/websockets", "docs/guides/callbacks", "docs/guides/https",
            "docs/guides/cors", "docs/guides/static-files", "docs/guides/publishing-api-documentation",
            "docs/guides/ci", "docs/guides/agents", "docs/guides/the-app-menu",
            "docs/guides/settings", "docs/guides/keymap", "docs/guides/themes-and-fonts",
          ],
        },
        {
          label: "Reference",
          items: [
            "docs/reference/server-file", "docs/reference/cli", "docs/reference/mcp-tools",
            "docs/reference/template-functions", "docs/reference/seed-generators",
            "docs/reference/keybindings", "docs/reference/action-names", "docs/reference/settings",
            "docs/reference/diagnostics", "docs/reference/performance",
          ],
        },
        { label: "Recipes", items: [{ autogenerate: { directory: "docs/recipes" } }] },
        { label: "Changelog", link: "/docs/changelog/" },
      ],
    }),
  ],
});
