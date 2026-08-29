# NumFlow UI/UX audit and redesign

Date: 2026-08-29
Scope: compact settings window, shared Slint controls, popup/menu surfaces, and transient HUD

## Audit summary

This second pass keeps the previous compact hierarchy but restores purposeful glass/material depth.
The main window now uses a Windows Mica backdrop when the Winit platform layer is available and a
high-opacity tinted fallback everywhere else. The fallback remains readable without compositor blur;
no platform is left with a fully transparent settings surface.

The audit used the UI/UX Pro Max guidance for glassmorphism, accessibility, focus states, contrast,
reduced motion, and density, then translated Apple HIG principles rather than copying macOS chrome.
The latest background pass adds one static diagonal graphite/navy glaze for depth; it is not animated
or strong enough to compete with the settings content.

## Findings addressed

- The previous fixed-height layout used `vertical-stretch` before the footer, creating an artificial
  empty region. The main window now binds height to the actual disclosure state: `354px` closed and
  `436px` open.
- The previous main surface was too flat after the opaque-surface pass. It now has a layered material
  model: window material, bounded highlight, subtle border, and controlled shadow.
- Active no longer carries a permanent bright blue outline. It uses a muted tinted surface, status
  marker, explicit Active/Inactive copy, switch, and On/Off text.
- Segmented controls now use one outer material boundary and a borderless accent-tinted selected
  layer; focus is kept as a separate keyboard ring.
- Slider tracks remain 4px, the interactive row remains 32px high, and the visible thumb is 16px.
- Popup/menu surfaces use the elevated glass token and a restrained elevation shadow without nested
  glass cards.
- HUD material and shadows now use shared semantic tokens instead of scattered rgba values.

## UX architecture

1. Active / Inactive master state.
2. Mouse button segmented control.
3. Pointer profile segmented control.
4. Speed and Acceleration sliders with right-aligned values.
5. Advanced progressive disclosure.
6. HUD and Bindings as secondary footer actions.

The runtime remains the source of truth. The UI only emits existing callbacks and reflects the
authoritative runtime snapshot; input hooks, Num Lock synchronization, mouse hold/release, tray,
sleep/resume, and session lifecycle were not changed.

## Material token system

`ui/design-system.slint` owns the shared semantic tokens:

- Window: `window-material`, `window-material-fallback`.
- Background: `background-glaze`.
- Surfaces: `surface-glass`, `surface-glass-elevated`, `surface-glass-hover`,
  `surface-glass-pressed`.
- Controls: `control-surface`, `control-hover`, `control-selected`.
- Content/state: `text-primary`, `text-secondary`, `text-tertiary`, `accent`, `accent-muted`,
  `focus-ring`, `status-active`, `destructive`.
- Geometry: `spacing-xs/sm/md/lg`, `radius-small/medium/large`, `control-height`, `row-height`,
  `label-column-width`, `value-column-width`.
- Depth: `border-subtle`, `separator`, `shadow-window`, `shadow-elevated`,
  `shadow-window-blur`, `shadow-elevated-blur`, `glass-highlight`.

Opacity is deliberate: the Windows-backed material is `0.84`, while the compositor-independent
fallback is `0.93`. These are implementation tokens, not per-component decoration. The surface is
opaque enough to prevent readable text behind NumFlow while still allowing the platform material to
provide depth.

## HIG and platform adaptation

- Hierarchy, harmony, consistency, clear grouping, progressive disclosure, readable text, visible
  focus, and state communication beyond color follow Apple HIG guidance.
- Windows uses the existing Winit `BackdropType::MainWindow` integration for Mica-like backdrop
  material. The surface falls back to a tinted high-opacity brush if the platform accessor is not
  available.
- macOS/Linux use the same interaction architecture and system-palette-driven controls without
  Windows font forcing or Apple-only traffic-light chrome. Their current platform backend boundary
  receives the robust fallback until native vibrancy/compositor support is implemented.
- Blur is used as platform material support for the main window and remains non-essential to UX;
  popups/HUD use bounded elevated surfaces rather than transparent full-window layers.

## Accessibility review

- Active state is communicated through text, switch state, On/Off copy, and marker shape—not color
  alone.
- Segmented controls expose radio semantics, selected state, keyboard activation, and a visible focus
  boundary.
- Toggles expose switch semantics, accessible labels, keyboard activation, and reduced-motion-aware
  thumb movement.
- Sliders preserve accessible labels, step keyboard navigation, a 32px hit area, 16px thumb, and
  visible focus ring.
- Advanced exposes button semantics and animates only height/opacity at 160ms, disabled when reduced
  motion is requested.
- System `Palette` keeps light/dark adaptation in one place; the high-opacity fallback protects
  contrast when blur/transparency is unavailable.

Remaining manual checks: Windows High Contrast, reduced-transparency preference, screen readers,
macOS accessibility settings, and 100/125/150/200% scaling on physical Windows/macOS/Linux hosts.
The repository's existing release checklist remains authoritative for runtime and desktop behavior.
