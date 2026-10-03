# UI consistency and iconography polish

## Goal

Polish Flux's existing dark operational interface so spacing, component dimensions, borders, typography, controls, scrollbar, and icons behave as one coherent visual system. The work preserves the current Signal Blueprint direction and does not redesign the route workflow or change DMX behavior.

## Visual system

Use a shared spacing scale of 4, 8, 12, 16, 24, and 32 pixels. The title strip stays 48 pixels high. Standard action buttons are 34 pixels high, inputs are 36–37 pixels high, and compact icon-only controls have an explicit square hit area.

The route workspace remains the primary surface. Its outer boundary, endpoint panels, inspector divider, activity row, and settings dialog use a consistent graphite border family. Endpoint panels use one medium radius; the route workspace remains structurally square to read as an operational canvas. Typography keeps a clear hierarchy: route name, section heading, metadata label, and value. Status colour always remains paired with its text label.

## Icon library

Add `lucide-react` as the only icon source for the React application. Replace handwritten SVG paths with Lucide icons:

- `Settings` for route configuration;
- `Minus`, `Square`, and `X` for native window actions;
- `Monitor`, `RotateCw`, and `X` for monitor, reconnect, and close actions where applicable.

Icons use a 16-pixel visual size for standard controls and 18 pixels for title-strip controls. Every icon-only button retains an accessible name and tooltip. No component contains custom SVG path data after the change.

## Components and interaction states

Consolidate button variants into title-strip, icon-only, secondary, primary, and attention actions. They share focus treatment, disabled opacity, transition timing, and border logic appropriate to their hierarchy. Hover emphasis stays subtle except for the close-to-tray and recovery actions, which retain their semantic red treatment.

The React-owned workspace scrollbar remains bounded below the title strip. Its track, thumb, hover state, width, and inset align with the same spacing and border tokens. It only appears when workspace content overflows.

## Scope

- `ui/package.json` and lockfile: add the icon dependency.
- `ui/src/main.tsx`: replace inline SVGs with Lucide components and assign shared button classes.
- `ui/src/style.css`: establish the spacing, dimensions, and component-state rules; refine the operational panels and custom scrollbar.

Tauri commands, capabilities, window behavior, settings persistence, runtime snapshots, and tray behavior are out of scope.

## Verification

1. Build TypeScript and Vite successfully.
2. Confirm no handwritten `<svg>` markup remains in the UI source.
3. Inspect normal, hover, focus, disabled, live, waiting, and error states at desktop and minimum supported window sizes.
4. Confirm title-strip controls, settings modal, monitor launch, reconnect action, and custom workspace scrollbar remain functional.
