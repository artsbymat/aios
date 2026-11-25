---
name: "AIOS (All in One Search)"
version: "0.1.0"
platform: "wayland-linux-gpui"
format: "figcraft-design-md"
dials:
  energy: 2
  rhythm: 2
  motion: 1
themes:
  supported: ["dark", "light", "system"]
  default: "system"
  systemSource: "org.freedesktop.portal.Settings (org.freedesktop.appearance.color-scheme)"

colors:
  dark:
    bg:
      canvas: "#131517"
      canvasTranslucent: "rgba(19, 21, 23, 0.94)"
      elevated: "#1B1E22"
      subtle: "#171A1D"
      selected: "#25292E"
      hover: "#1F2328"
      keycap: "#22262B"
      scrim: "rgba(10, 12, 14, 0.64)"
    text:
      primary: "#F1EFEA"
      secondary: "#A3A8AE"
      muted: "#858B93"
      onAccent: "#131517"
    border:
      subtle: "#2A2F36"
      default: "#363C44"
      strong: "#656D76"
      focus: "#EE7B4D"
    accent:
      solid: "#E87143"
      fg: "#EE7B4D"
      fgOnSelected: "#F28B61"
      subtleBg: "rgba(232, 113, 67, 0.14)"
    status:
      success: "#4CC38A"
      successBg: "rgba(76, 195, 138, 0.14)"
      warning: "#E5B546"
      warningBg: "rgba(229, 181, 70, 0.14)"
      danger: "#F26D6D"
      dangerBg: "rgba(242, 109, 109, 0.14)"

  light:
    bg:
      canvas: "#F6F4EF"
      canvasTranslucent: "rgba(246, 244, 239, 0.95)"
      elevated: "#FFFFFF"
      subtle: "#EFECE5"
      selected: "#E8E4DC"
      hover: "#ECE8E1"
      keycap: "#E9E5DD"
      scrim: "rgba(24, 26, 28, 0.36)"
    text:
      primary: "#181A1C"
      secondary: "#4F555C"
      muted: "#5E656D"
      onAccent: "#FFFFFF"
    border:
      subtle: "#DFDAD0"
      default: "#C8C2B6"
      strong: "#7B828A"
      focus: "#B54116"
    accent:
      solid: "#B54116"
      fg: "#B54116"
      fgOnSelected: "#A43A12"
      subtleBg: "rgba(181, 65, 22, 0.11)"
    status:
      success: "#176B45"
      successBg: "rgba(23, 107, 69, 0.11)"
      warning: "#8A5600"
      warningBg: "rgba(138, 86, 0, 0.12)"
      danger: "#B91C1C"
      dangerBg: "rgba(185, 28, 28, 0.11)"

typography:
  fontFamily:
    sans: "IBM Plex Sans, Noto Sans, Cantarell, sans-serif"
    mono: "IBM Plex Mono, Noto Sans Mono, monospace"
  scale:
    searchInput: { size: "16px", lineHeight: "24px", weight: 400 }
    viewTitle: { size: "15px", lineHeight: "20px", weight: 600 }
    itemTitle: { size: "13px", lineHeight: "18px", weight: 500 }
    itemSubtitle: { size: "12px", lineHeight: "16px", weight: 400 }
    sectionHeader: { size: "11px", lineHeight: "16px", weight: 600 }
    bodyMarkdown: { size: "13px", lineHeight: "20px", weight: 400 }
    codeBlock: { size: "12px", lineHeight: "18px", weight: 400 }
    keycap: { size: "11px", lineHeight: "14px", weight: 500 }
    footerMeta: { size: "12px", lineHeight: "16px", weight: 500 }
  features:
    tabularNumbers: "tnum"

spacing:
  unit: "4px"
  xxs: "2px"
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"

rounded:
  none: "0px"
  xs: "4px"
  sm: "6px"
  md: "8px"
  lg: "10px"
  window: "12px"

layout:
  window:
    width: "750px"
    minHeightCompact: "56px"
    defaultHeight: "480px"
    maxHeight: "620px"
    topOffsetFromMonitorEdge: "18%"
  header:
    height: "52px"
    paddingX: "16px"
  footer:
    height: "40px"
    paddingX: "14px"
  listRow:
    height: "40px"
    paddingX: "10px"
    gap: "10px"
  splitDetail:
    listWidthRatio: "40%"
    detailWidthRatio: "60%"
  metadataSidebar:
    width: "240px"
  actionPanel:
    width: "320px"
    maxHeight: "360px"

elevation:
  planes:
    ground: "none"
    popover: "0 10px 28px rgba(0, 0, 0, 0.34)"
    window: "0 18px 48px rgba(0, 0, 0, 0.44)"
  blur:
    allowedTargets: ["root-layer-shell-window"]
    maxTargets: 1
    minOpaqueFallbackAlpha: 0.92

motion:
  reducedMotionRespect: true
  popoverTransitionMs: 100
  toastTransitionMs: 120
  selectionTransitionMs: 0
---

# AIOS (All in One Search) Design Specification

**Design Read:** Reading this as a Wayland desktop command palette and multi-view plugin workbench for Linux power users, in a tactile mineral-neutral utilitarian style, dial ENERGY 2 / RHYTHM 2 / MOTION 1.

**Dial:** ENERGY 2 / RHYTHM 2 / MOTION 1

---

## Overview

AIOS (All in One Search) is a keyboard-first, local-first command launcher and multi-view plugin workbench written in Pure Rust using GPUI for Wayland compositors (Hyprland and Niri). Its visual language draws from precision drafting instruments and Swiss mineral surfaces rather than consumer web apps or neon developer templates.

Core principles that govern every screen:

1. **Keyboard Speed Over Visual Noise**: Every view must present one unmistakable focal point (the active search input, the selected row/tile, or the active form field) with zero layout shift between keystrokes.
2. **Dual-Mode Parity (Dark, Light, System)**: Light mode is a first-class warm alabaster surface, not an inverted afterthought. Dark mode is a deep mineral basalt surface, not pitch black. Both follow `org.freedesktop.portal.Settings` when set to System.
3. **Strict Anti-Purple & Anti-Slop Discipline**: No purple, indigo, magenta, or blue-cyan AI gradients anywhere in the UI. Color is restrained to neutral mineral tones plus a single Signal Copper accent.
4. **Approved Brand Mark (`Modular Panes · Left Focus`)**: The official AIOS symbol combines a top-left Signal Copper focal circle (`#E87143` dark / `#B54116` light) with three Basalt/Alabaster modular Wayland panes (search bar + 40/60 split views), paired with the custom geometric outlined `AIOS` wordmark. Master files live in `assets/logo/` (see `assets/logo/BRAND_GUIDELINES.md`). Do not substitute stock sparkle or magnifying-glass icons.

---

## Colors

The active palette uses **two neutral families** (mineral basalt for Dark, warm alabaster for Light) plus **one deliberate accent** (Signal Copper). Semantic colors (`success`, `warning`, `danger`) appear strictly when signaling real application states (toast outcomes, destructive actions, field validation errors), always paired with text and a functional icon.

### Dark Mode Palette (Basalt & Signal Copper)

- **`bg.canvas` (`#131517`)**: Primary launcher window background. Deep warm charcoal reduces glare on OLED and IPS monitors while preserving crisp contrast against `bg.elevated`.
- **`bg.elevated` (`#1B1E22`)**: Action Panel popover, Confirm Alert modal, dropdown menus, and Detail metadata surfaces.
- **`bg.selected` (`#25292E`)**: Active list row or grid tile highlight, paired with a 2px left Signal Copper selection rail.
- **`bg.keycap` (`#22262B`)**: Recessed background for keyboard shortcut tokens (`Enter`, `Ctrl+K`, `Tab`).
- **`text.primary` (`#F1EFEA`)**: Warm off-white primary copy and item titles (`15.93:1` on `bg.canvas`).
- **`text.secondary` (`#A3A8AE`)**: Subtitles, section headers, and accessories (`7.64:1` on `bg.canvas`).
- **`text.muted` (`#858B93`)**: Input placeholders, timestamps, and secondary keycap labels (`5.33:1` on `bg.canvas`).
- **`accent.solid` (`#E87143`) & `accent.fg` (`#EE7B4D`)**: Signal Copper reserved exclusively for matched fuzzy character highlights, active selection rail, primary form submit button, and `:focus-visible` rings.

### Light Mode Palette (Alabaster & Signal Copper)

- **`bg.canvas` (`#F6F4EF`)**: Warm mineral paper surface that avoids harsh pure-white glare on bright desktop setups.
- **`bg.elevated` (`#FFFFFF`)**: Popovers, modals, and form input fields that sit one layer above the canvas.
- **`bg.selected` (`#E8E4DC`)**: Active row or grid tile selection surface, paired with the 2px Signal Copper rail.
- **`bg.keycap` (`#E9E5DD`)**: Tactile recessed keycap surface.
- **`text.primary` (`#181A1C`)**: Deep mineral ink (`15.88:1` on `bg.canvas`).
- **`text.secondary` (`#4F555C`)**: Subtitles and metadata (`6.86:1` on `bg.canvas`).
- **`text.muted` (`#5E656D`)**: Placeholders and secondary hints (`5.37:1` on `bg.canvas`).
- **`accent.solid` / `accent.fg` (`#B54116`)**: Deep kiln copper tuned specifically for light-surface legibility (`5.12:1` on `bg.canvas`, `5.63:1` with white text on solid).

### Verified WCAG AA Contrast Matrix

All pairings are checked with WCAG 2.x luminance math (4.5:1 normal text, 3.0:1 non-text UI boundaries):

| Token Pairing                          | Dark Ratio | Light Ratio | Threshold  |
| -------------------------------------- | ---------- | ----------- | ---------- |
| `text.primary` on `bg.canvas`          | 15.93:1    | 15.88:1     | 4.5:1 PASS |
| `text.secondary` on `bg.canvas`        | 7.64:1     | 6.86:1      | 4.5:1 PASS |
| `text.muted` on `bg.canvas`            | 5.33:1     | 5.37:1      | 4.5:1 PASS |
| `text.primary` on `bg.selected`        | 12.73:1    | 13.76:1     | 4.5:1 PASS |
| `accent.fg` on `bg.canvas`             | 6.60:1     | 5.12:1      | 4.5:1 PASS |
| `accent.fgOnSelected` on `bg.selected` | 6.02:1     | 5.19:1      | 4.5:1 PASS |
| `text.onAccent` on `accent.solid`      | 5.99:1     | 5.63:1      | 4.5:1 PASS |
| `border.strong` on `bg.canvas`         | 3.49:1     | 3.54:1      | 3.0:1 PASS |
| `status.success` on `bg.canvas`        | 8.26:1     | 5.92:1      | 4.5:1 PASS |
| `status.warning` on `bg.canvas`        | 9.61:1     | 5.60:1      | 4.5:1 PASS |
| `status.danger` on `bg.canvas`         | 6.26:1     | 5.89:1      | 4.5:1 PASS |

---

## Typography

- **Primary Sans (`IBM Plex Sans`)**: Chosen for open apertures, crisp horizontal stems at 12px to 15px under Wayland fractional scaling (1.25x, 1.5x), and a technical character distinct from generic web defaults. Falls back cleanly to `Noto Sans` or `Cantarell`.
- **Monospace (`IBM Plex Mono`)**: Used strictly for functional structured data: keyboard shortcut keycaps, calculator outputs, file paths, hex/RGB color values, process PIDs, and Markdown code blocks. Never used as oversized decorative headlines.
- **Tabular Numerals (`tnum`)**: Enabled on all numeric accessories (CPU%, RAM, timestamps, calculator values) so columns align vertically during rapid scanning.
- **Fuzzy Match Highlighting**: Characters indexed by `aios-fuzzy` (`MatchResult.indices`) render in `font-weight: 600` + `accent.fg` (or `accent.fgOnSelected` on selected rows) with a subtle underline cue on high-contrast/colorblind mode so matches remain legible without relying on hue alone.

---

## Layout

All layout rules map directly to FigCraft Auto Layout and GPUI Flexbox (`flex()`, `flex_row()`, `flex_col()`, `w_full()`, `flex_1()`, `shrink_0()`). Hardcoded absolute coordinates are forbidden except for anchored popovers (`ActionPanel`, `Dropdown`) and the centered `ConfirmAlert` modal.

### Window Shell Anatomy

```text
+-----------------------------------------------------------------------+
| [Back]  [Search Icon / Command Icon]  Search input... [Args] [Filter] |  52px (Fixed)
+-----------------------------------------------------------------------+
|                                                                       |
|  Central Plugin Viewport (Fill remaining height, scrollable)          |  388px..528px
|  (List | List+Detail | Grid | Detail+Metadata | Form | Empty/Error)   |  (Fill)
|                                                                       |
+-----------------------------------------------------------------------+
| [Plugin Icon] Context / Toast      [Primary Action ↵] | [Actions ^K]  |  40px (Fixed)
+-----------------------------------------------------------------------+
```

### Spatial & Sizing Rules

- **Grid System**: Strict `4px` micro-unit, `8px` standard rhythm.
- **Root Window**: Fixed `750px` width on standard desktop monitors (clamped to `92%` of monitor width on narrow displays). Height adapts cleanly: `56px` in Compact empty mode, `480px` standard expanded height, up to `620px` for dense `Detail` or `Form` views.
- **Horizontal Padding**: `16px` in header and detail prose, `8px` outer inset on list containers so selection pills have visible breathing room (`10px` internal row padding).
- **Split View (`List + Detail`)**: `40%` left list (`min 280px`) separated by a crisp `1px` `border.subtle` divider from `60%` right Markdown + Metadata detail pane.
- **Detail + Metadata View**: Main Markdown canvas fills remaining width (`flex_1`); right Metadata sidebar fixed at `240px` (`shrink_0`) with `16px` padding and key-value rows stacked vertically.
- **Grid View**: CSS/Flex wrap grid supporting `1` to `8` columns configured by the plugin, consistent `10px` gap, aspect-ratio locked tiles so images never trigger layout shifts.
- **Form View**: Single-column vertical stack, `max-width: 580px` centered inside the viewport, visible left-or-top labels (never placeholder-only fields), `14px` vertical field gap, `4px` label-to-input gap, and inline error text directly beneath invalid fields.

---

## Elevation & Depth

AIOS uses **flat structural layering** separated by crisp `1px` borders, reserving drop shadows strictly for true z-plane separations:

1. **Plane 0 (Internal Views & Controls)**: Zero drop shadows. Hierarchy comes from `bg.canvas` vs `bg.subtle` vs `bg.selected` and `1px` `border.subtle` dividers.
2. **Plane 1 (Root Layer-Shell Overlay)**: Single outer elevation shadow (`elevation.planes.window`) + `1px` `border.default` hairline outline so the floating window separates crisply from busy wallpapers or tiled terminal windows on Hyprland and Niri.
3. **Plane 2 (Popovers & Modals)**: `ActionPanel`, `List.Dropdown`, and `ConfirmAlert` sit on `bg.elevated` with `elevation.planes.popover` and `1px` `border.default` border.
4. **Backdrop Blur Dose Cap**: Optional compositor blur applies **only** to the single root layer-shell surface (`namespace = "aios"`). It must stay at `>= 0.92` opacity (`canvasTranslucent`) so foreground contrast never degrades below WCAG AA over bright windows. Internal panels remain 100% opaque.
5. **No Decorative Glow**: Focus states use solid `2px` `border.focus` rings, never blurred colored shadows.

---

## Shapes

Border radii express component hierarchy rather than uniform rounding:

- **Root Window**: `12px` (`rounded.window`)
- **Overlay Popovers & Modals (`ActionPanel`, `ConfirmAlert`, `HUD`)**: `10px` (`rounded.lg`)
- **Grid Tiles & Inline Calculator Result Card**: `8px` (`rounded.md`)
- **List Rows, Form Inputs, Buttons, Dropdowns**: `6px` (`rounded.sm`)
- **Keycap Tokens, Inline Argument Pills, Status Tags**: `4px` (`rounded.xs`)
- **Active Selection Rail**: `2px` wide vertical bar inset on the left edge of the selected row/tile (`rounded.xs`).

Pill shapes (`9999px`) are reserved exclusively for circular progress spinners and user-defined circular avatars if a plugin explicitly requests `Mask::Circle`.

---

## Components

Every interactive component defines five explicit states: `default`, `hover`, `active/pressed`, `selected`, `focus-visible`, and `disabled` (with `cursor: pointer` on enabled controls and `cursor: not-allowed` on disabled controls).

### 1. Header Search Bar & Inline Arguments

- Left slot: Back button (`←`, `Esc` / `Backspace` when query empty) when navigation depth > 1; otherwise current view icon.
- Center slot: Unbordered full-flex text input (`searchInput` 16px). When a selected command declares inline arguments, `Tab` activates crisp `4px`-radius argument pill inputs inline right next to the command badge.
- Right slot: Optional `List.Dropdown` / `Grid.Dropdown` filter selector with chevron and `Ctrl+P` shortcut hint.
- Loading state: Thin `2px` deterministic/indeterminate horizontal progress bar running cleanly along the `1px` bottom divider of the header (zero layout shift, stops cleanly under reduced motion).

### 2. List Row (`List.Item`)

- Height: `40px` (`listRow.height`), horizontal flex (`Hug` icon + `Fill` title/subtitle + `Hug` accessories).
- Selected state: `bg.selected` fill + `2px` left `accent.solid` vertical rail (functional selection signal).
- Accessories: Right-aligned cluster (`gap: 8px`) supporting text, bordered status tags (color + icon + text), relative dates (`tnum`), and icon tooltips.

### 3. Inline Calculator Banner (Root Search Special Widget)

- Appears as the top pinned item in Root Search only when the input parses as a valid expression, unit conversion, currency query, or date/timezone math.
- Two-column horizontal split: Left shows parsed normalized input (`IBM Plex Mono` 13px, `text.secondary`); Right shows computed result (`IBM Plex Mono` 20px SemiBold, `text.primary`, `tnum`) with a `Copy Result` primary action.

### 4. Grid Tile (`Grid.Item`)

- Square or aspect-locked visual frame on `bg.subtle` with `1px` `border.subtle`.
- Selected state: `2px` `border.focus` outline + `bg.selected` footer caption bar.

### 5. Detail & Metadata View (`Detail`)

- Markdown typography with clear spacing hierarchy (H1 `18px`, H2 `15px`, H3 `14px`, body `13px/20px`), `bg.subtle` code blocks with `1px` `border.subtle` and syntax colors meeting 4.5:1 contrast.
- Metadata sidebar rows show label (`text.muted`, `11px`) stacked or aligned with value (`text.primary`, `12px`), clickable links underlined on hover/focus, and compact tag lists.

### 6. Form Controls (`Form`)

- Every input has a persistent visible `<label>` (never placeholder-only).
- Focus state: `2px` solid `border.focus` ring (`>= 5.1:1` contrast).
- Error state: `status.danger` border + warning icon + concrete error message directly below the control.

### 7. Action Footer & Action Panel (`Ctrl+K`)

- Footer bar (`40px`): Left shows active plugin icon + title (temporarily replaced by active `Toast` when triggered). Right shows Primary Action label + `[↵]` keycap, a `1px` vertical divider, and `Actions` + `[Ctrl+K]` keycap.
- Action Panel popover: Anchored bottom-right above the footer (`320px` wide, `maxHeight: 360px`), contains its own bottom-anchored mini filter input, grouped `ActionPanel.Section` lists, submenu drill-in (`→`), and right-aligned shortcut keycaps. Destructive actions use `status.danger` icon and label.

### 8. Empty, Loading, and Error Views (Mandatory on All Data Views)

- **Empty State**: Centered concise explanation stating _why_ it is empty and the immediate next keyboard action (e.g., `"No matching clipboard entries for 'nginx'. Press Esc to clear filter."`), never bare `"No data"`.
- **Loading State**: Header 2px progress track plus descriptive text when the canvas is empty (e.g., `"Indexing desktop applications..."`).
- **Error State**: Explicit failure reason + diagnostic detail + actionable recovery button (`Retry`, `Open Plugin Logs`, or `Configure Preferences`).

### 9. Keycap Token (Identity Motif)

- Compact recessed pill (`rounded.xs`, `bg.keycap`, `1px` `border.default`, subtle `1px` bottom edge bevel) rendering modifier and key glyphs in `IBM Plex Mono` `11px`.
- Used consistently across Footer, Action Panel, Command Arguments, and Settings Shortcut Recorder.

---

## Motion & Interaction

AIOS runs at **MOTION 1**:

- **Zero idle or looping animations**: No pulsing dots, bouncing icons, or floating cards.
- **Instant list/grid selection (`0ms`)**: Arrow keys and `Ctrl+J`/`Ctrl+K` (Vim-friendly optional bindings) move the selection rail instantaneously.
- **Micro-transitions (`100ms` ease-out)**: Applied strictly to `ActionPanel` popover reveal, `ConfirmAlert` scrim, and `Toast` appearance to preserve spatial orientation.
- **Reduced Motion**: When system or user reduced-motion setting is active, all transitions drop to `0ms` and loading progress bars use a static labeled status indicator.
- **Full Keyboard Operability**:
  - `Up`/`Down` or `Ctrl+N`/`Ctrl+P`: Navigate list/action items.
  - `Left`/`Right`/`Up`/`Down`: Navigate `Grid` tiles.
  - `Enter`: Trigger Primary Action; `Ctrl+Enter`: Trigger Secondary Action.
  - `Ctrl+K`: Open/Close Action Panel.
  - `Tab` / `Shift+Tab`: Move between inline command arguments, form fields, and filter dropdown.
  - `Esc`: Close popover/modal first -> clear search query -> pop view stack -> hide launcher window.

---

## Design Decisions Rationale (R-31)

- **Why Basalt + Warm Alabaster neutrals**: Pure `#000000` causes halation on high-contrast displays and pure `#FFFFFF` causes glare; mineral-tinted neutrals maintain >15:1 contrast while staying comfortable across long sessions.
- **Why Signal Copper (`#E87143` / `#B54116`)**: Avoids generic AI purple/cyan gradients and Raycast's coral-red, creates an unmistakable industrial Rust-workbench identity, and passes WCAG AA in both Dark and Light modes.
- **Why IBM Plex Sans + IBM Plex Mono**: Delivers superior small-size legibility on Wayland fractional-scaled screens and avoids default AI typeface picks (Inter/Geist/Space Grotesk).
- **Why `750px` fixed launcher width**: Keeps line length readable in split Detail/Markdown views (~65ch) while staying compact enough to float cleanly over tiled Hyprland and Niri workspaces.
- **Why 2px left rail on selected rows**: Functions as a high-precision active-state signal paired with background contrast (`bg.selected`), ensuring instant scanability even for color-vision-deficient users.
- **Why `MOTION 1` (0ms selection, 100ms overlays)**: A keyboard launcher is an instrument invoked hundreds of times per day; animation latency directly degrades perceived speed.
- **Why Keycap Tokens as the identity motif**: Reinforces keyboard-first muscle memory on every screen without adding non-functional decoration.

---

## Do's and Don'ts (FigCraft & Agent Constraints)

- **Do** derive every color, radius, spacing, and typography value in GPUI directly from the theme tokens defined in this file (`ThemeColors`, `ThemeSpacing`, `ThemeRadii`).
- **Do** build every view and component to function consistently in all three theme modes (`Dark`, `Light`, `System`) and verify contrast in both Light and Dark.
- **Do** enforce explicit Empty, Loading, and Error states on every plugin view (`List`, `Grid`, `Detail`, `Form`).
- **Do** pair every color status signal (`success`, `warning`, `danger`) with a clear text label and a functional icon.
- **Don't** introduce purple, violet, indigo, or blue-cyan gradients anywhere in the shell, defaults, or built-in plugins.
- **Don't** apply backdrop blur to internal panels, cards, or popovers (cap at the single root layer-shell window).
- **Don't** make every element pill-shaped or add drop shadows to list rows, grid tiles, or form inputs.
- **Don't** ship dead buttons, placeholder-only form fields, decorative emojis in UI chrome, or fabricated sample data in real views.
- **Don't** hardcode absolute pixel coordinates for child layouts; use GPUI flex / FigCraft Auto Layout (`Hug`, `Fill`, `Fixed`) exclusively.
