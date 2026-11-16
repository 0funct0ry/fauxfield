import { getCollection } from "astro:content";
import { OGImageRoute } from "astro-og-canvas";

const docs = await getCollection("docs");
const pages = Object.fromEntries(
  docs.map((d) => [d.id.replace(/\/index$/, ""), { title: d.data.title, description: d.data.description ?? "" }]),
);
pages["index"] = {
  title: "Mock the API your frontend is waiting for.",
  description: "Local-first mock API server: GPU desktop app, headless runner, MCP server.",
};

export const { getStaticPaths, GET } = await OGImageRoute({
  pages,
  getImageOptions: (_path, page) => ({
    title: page.title,
    description: page.description,
    bgGradient: [[40, 44, 51]],
    border: { color: [116, 173, 232], width: 12, side: "inline-start" },
    padding: 64,
    font: {
      title: { color: [220, 224, 229], size: 64, weight: "SemiBold", families: ["sans-serif"] },
      description: { color: [169, 175, 188], size: 30, families: ["sans-serif"] },
    },
  }),
});
