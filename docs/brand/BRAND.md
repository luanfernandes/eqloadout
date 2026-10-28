# EQ Loadout brand

## Name

- Written **EQ Loadout**: two words, "EQ" in capitals.
- Use `eqloadout` (lowercase, one word) where spaces don't work: the exe, the repo, the config folder, domains.
- The idea: every game gets its own EQ "loadout", the way it gets its own gear in-game.

## Mark

![logo](logo-256.png)

A rounded-square inventory slot holding three mixer faders. The knob positions draw a small EQ curve. The mark is built on a 32×32 grid with even coordinates, so it stays sharp at 16 px in the tray.

- Source: [`logo.svg`](logo.svg). Don't redraw it, recolor the tile, or add effects (glow, gradient, outline).
- Keep at least 1/4 of the mark's width as clear space around it.
- Smallest size: 16 px.

## Colors

| Token | Hex | Use |
|---|---|---|
| Ink | `#0E1015` | Page and banner background |
| Slot | `#151820` | Tile fill |
| Rim | `#3A4254` | Tile border, fader tracks |
| Signal | `#A6FF3D` | Knobs, the "EQ" in the wordmark. The one accent. |
| Text | `#E8ECF4` | Wordmark, headings on dark |
| Muted | `#9AA3B5` | Taglines, secondary text |
| Paused | `#7D8597` | Knobs in the paused state |
| Warning | `#FFB020` | Knobs in the warning state |

Signal green is for the mark and small highlights only, never for large areas or body text.

## Type

- Wordmark and headings: **Chakra Petch** SemiBold (Google Fonts, OFL).
- Body and taglines: **Inter** Regular.
- Inside the app: Segoe UI, the Windows system font. The app uses native controls and does not ship fonts.

## Tray states

The tray icon only changes knob color, so the shape stays recognizable:

| State | Icon | When |
|---|---|---|
| Active | `res/eqloadout.ico` (green) | Following focus normally |
| Paused | `res/paused.ico` (grey) | Paused from the tray menu |
| Needs attention | `res/warning.ico` (amber) | A profile file is missing or game.txt can't be written. The tooltip says which. |

## Files

| File | What |
|---|---|
| `logo.svg`, `tray-paused.svg`, `tray-warning.svg` | Sources |
| `logo-256.png` | Raster logo |
| `banner.html` → `banner.png` (1280×400) | README header |
| `banner.html` → `social.png` (1280×640) | GitHub social preview (Settings → Social preview) |

To regenerate the `.ico` files and the PNGs after editing a source (needs Microsoft Edge):

```
powershell -ExecutionPolicy Bypass -File res\make-icons.ps1
```
