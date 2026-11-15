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
