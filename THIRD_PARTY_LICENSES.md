# Third-party licences

Fauxfield is MIT licensed. The files below ship inside the GUI build (`crates/fauxfield-gui/assets/`) and keep their own licences. They are unchanged upstream files; the headless build embeds none of them.

## Theme JSON (Zed theme format, unchanged)

| Files | Source | Licence |
|---|---|---|
| `themes/one.json` | zed-industries/zed `assets/themes/one` | see `LICENSES` in zed-industries/zed (One Dark/One Light, MIT) |
| `themes/ayu.json` | zed-industries/zed `assets/themes/ayu` | MIT (Ayu) |
| `themes/gruvbox.json` | zed-industries/zed `assets/themes/gruvbox` | MIT (Gruvbox) |
| `themes/rose-pine*.json` | rose-pine/zed | MIT |
| `themes/catppuccin.json` | catppuccin/zed (`catppuccin-mauve.json`) | MIT |
| `themes/tokyo-night.json` | ssaunderss/zed-tokyo-night | MIT |
| `themes/nord.json` | mikasius/zed-nord-theme | MIT |

Solarized is not bundled yet: the only Zed port found (harmtemolder/Solarized.zed) is GPL-3.0, which does not fit an MIT product. See SPEC §20.34.

## Fonts (SIL Open Font License 1.1)

| Files | Source | Licence text |
|---|---|---|
| `fonts/IBMPlexSans_*.ttf`, `IBMPlexSans-Italic.ttf` | IBM/plex | `fonts/OFL-IBMPlexSans.txt` |
| `fonts/IBMPlexMono-*.ttf` | IBM/plex | `fonts/OFL-IBMPlexMono.txt` |

## Swagger UI (Apache-2.0)

The headless and GUI builds embed three files of `swagger-ui-dist` 5.34.1 (npm, published 2026-10-30), stored gzip-compressed in `crates/fauxfield-openapi/assets/` for the API documentation page (SPEC §12.4, §20.35). Copyright 2020-2021 SmartBear Software Inc.

| Files | Source | Licence text |
|---|---|---|
| `swagger-ui-bundle.js.gz`, `swagger-ui.css.gz`, `favicon.png.gz` (from `favicon-32x32.png`) | https://registry.npmjs.org/swagger-ui-dist/-/swagger-ui-dist-5.34.1.tgz | `assets/swagger-ui.LICENSE`, `assets/swagger-ui.NOTICE`, and the bundled-dependency notices in `assets/swagger-ui-bundle.js.LICENSE.txt` |

SHA-256 of the uncompressed upstream files: `swagger-ui-bundle.js` `36eeb4d5640b9c036b1d0f854499af1cceebbdb260b790a03a18be5e8488fbf5`; `swagger-ui.css` `c3cfe8248c4b0b89673600fb829924eafbac6de4bc4b2b06f5f5243483074d0a`; `favicon-32x32.png` `3ed612f41e050ca5e7000cad6f1cbe7e7da39f65fca99c02e99e6591056e5837`.
