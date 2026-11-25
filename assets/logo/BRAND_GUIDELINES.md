# AIOS Brand & Logo Guidelines

**Mark Name:** Modular Panes (Left Focus)  
**Mark Type:** Abstract Modular Workbench Symbol + Custom Geometric Outlined Wordmark

---

## 1. Logo Anatomy

The AIOS symbol abstracts the Wayland launcher architecture into four geometric primitives on a `256 x 256` integer grid:
- **Top-left focal circle (`cx=52, cy=52, r=28`)**: Search entry point in Signal Copper.
- **Top-right command bar (`132 x 56`)**: Persistent header search bar.
- **Bottom-left list pane (`76 x 132`, 40% width)**: Fuzzy-filtered result list.
- **Bottom-right detail pane (`112 x 132`, 60% width)**: Multi-view plugin canvas (`Detail`, `Grid`, `Form`).

All outer window corners use `r=24`, inner split corners use `r=8`, and channels are `20` units wide (`24` units on `aios-symbol-small.svg`). The `AIOS` wordmark uses geometric `<path>` outlines with zero font dependencies.

---

## 2. Official Dual-Mode Colors

| Role | Surface Mode | HEX | RGB | WCAG AA Contrast |
|---|---|---|---|---|
| Signal Copper (Focal Circle) | Dark (`#131517`) | `#E87143` | `232, 113, 67` | `5.99:1` PASS |
| Warm Off-White (Panes & Type) | Dark (`#131517`) | `#F1EFEA` | `241, 239, 234` | `15.93:1` PASS |
| Kiln Copper (Focal Circle) | Light (`#F6F4EF`) | `#B54116` | `181, 65, 22` | `5.12:1` PASS |
| Deep Mineral Ink (Panes & Type) | Light (`#F6F4EF`) | `#181A1C` | `24, 26, 28` | `15.88:1` PASS |
| Basalt App Icon Tile | Desktop / Tray | `#131517` | `19, 21, 23` | Primary container |

---

## 3. Clear Space, Minimum Sizes & Rules

- **Clear Space**: Keep a margin of at least `56u` (1x focal circle diameter, 22% of symbol width) on all sides.
- **Minimum Sizes**: `24px` for standard symbols, `16px` using `aios-symbol-small.svg`, and `96px` wide for horizontal lockups.
- **Do Not**: Mirror the mark horizontally, apply purple/cyan gradients or glows, alter channel spacing, or replace the outlined wordmark with a stock font.

---

## 4. Project Asset Inventory

- `assets/icons/aios.svg`: Scalable Wayland/Freedesktop app icon (`hicolor/scalable/apps`)
- `assets/icons/aios-64.png`, `aios-256.png`, `aios-512.png`: Standard Freedesktop PNG app icons
- `assets/logo/aios-symbol-dark.svg` & `aios-symbol-light.svg`: Dual-mode GPUI UI symbols
- `assets/logo/aios-symbol-small.svg`: 16-24px Wayland status bar & header badge cut
- `assets/logo/aios-symbol-mono.svg`: Monochrome symbolic icon for system trays
- `assets/logo/aios-lockup-dark.svg` & `aios-lockup-light.svg`: Horizontal lockups for README and About view
