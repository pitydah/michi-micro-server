---
name: Michi Micro Server
description: Adaptive glass for a private, listen-first music environment.
colors:
  acoustic-ink: "#090B10"
  fog: "#EFF2F4"
  pearl: "#F5F7FA"
  deep-ink: "#161A22"
  sea-glass: "#6FBFB5"
  warm-brass: "#D5A466"
  coral: "#E36B68"
  smoke-pane: "rgba(25, 29, 37, 0.72)"
  frost-pane: "rgba(255, 255, 255, 0.68)"
  smoke-edge: "rgba(255, 255, 255, 0.14)"
  frost-edge: "rgba(22, 26, 34, 0.14)"
  hero-frame-coral: "rgba(227, 107, 104, 0.78)"
  hero-frame-brass: "rgba(213, 164, 102, 0.88)"
  hero-equalizer: "rgba(227, 107, 104, 0.20)"
  hero-rings: "rgba(213, 164, 102, 0.34)"
typography:
  display:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "clamp(2.25rem, 5vw, 4.75rem)"
    fontWeight: 650
    lineHeight: 0.98
    letterSpacing: "-0.035em"
  headline:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "clamp(1.5rem, 2.5vw, 2.25rem)"
    fontWeight: 650
    lineHeight: 1.12
    letterSpacing: "-0.025em"
  title:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 650
    lineHeight: 1.3
  body:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "0.9375rem"
    fontWeight: 400
    lineHeight: 1.55
  label:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 600
    lineHeight: 1.3
  control:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "0.8125rem"
    fontWeight: 600
    lineHeight: 1.3
  body-small:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
  data:
    fontFamily: "ui-monospace, SFMono-Regular, Consolas, monospace"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.4
  metric:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "clamp(1.25rem, 2vw, 1.75rem)"
    fontWeight: 650
    lineHeight: 1.1
  modal-title:
    fontFamily: "Manrope, system-ui, sans-serif"
    fontSize: "1.4rem"
    fontWeight: 650
    lineHeight: 1.2
rounded:
  xs: "8px"
  control: "10px"
  compact: "11px"
  grouped: "12px"
  brand: "13px"
  content: "14px"
  toolbar: "16px"
  artwork: "18px"
  cover: "20px"
  structural: "22px"
  round: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "32px"
  xxl: "48px"
components:
  button-primary:
    backgroundColor: "{colors.warm-brass}"
    textColor: "{colors.deep-ink}"
    rounded: "{rounded.control}"
    padding: "10px 16px"
  button-secondary:
    backgroundColor: "transparent"
    textColor: "{colors.pearl}"
    rounded: "{rounded.control}"
    padding: "10px 16px"
  input:
    backgroundColor: "rgba(255, 255, 255, 0.06)"
    textColor: "{colors.pearl}"
    rounded: "{rounded.control}"
    padding: "10px 14px"
  structural-pane-dark:
    backgroundColor: "{colors.smoke-pane}"
    textColor: "{colors.pearl}"
    rounded: "{rounded.structural}"
  structural-pane-light:
    backgroundColor: "{colors.frost-pane}"
    textColor: "{colors.deep-ink}"
    rounded: "{rounded.structural}"
---

# Design System: Michi Micro Server

## Overview

**Creative North Star: "Adaptive Glass"**

Michi is a private listening environment made from smoked acoustic glass at night and frosted mineral glass in daylight. The same topology adapts to ambient light: dark mode is **Smokeglass**, light mode is **Frostglass**. Artwork and active music provide the emotional color while the product chrome recedes.

Glass is architecture, not decoration. It defines the sidebar, top bar, listening rail, mobile player, modal, and toast. Music content rests on clear or softly tonal planes so browsing remains fast, legible, and lightweight. The orange headphone cat appears only during welcome, pairing, or empty states, seated naturally inside a quiet glass scene.

The dashboard welcome hero is Michi's single cinematic brand image: a photorealistic orange tabby in black headphones, warm Brass/Coral rim light, restrained Sea Glass fill, and a Smokeglass acoustic background. The cat occupies the left third while factual HTML playback copy uses the right-side negative space. The raster never contains typography, logos, controls, frames, or watermarks.

**Key Characteristics:**
- Smoked or milky structural panes with visible cool edges and restrained refraction.
- A listen-first composition led by cover art, title, artist, progress, and queue.
- Warm Brass signals active playback and primary action; Sea Glass signals connection and secondary state.
- Human sentence-case typography; monospace is limited to durations, IDs, and measurements.
- Material depth uses offset soft shadows and one restrained specular edge, never colored halos.

## Colors

The palette is neutral at rest and music-derived in motion. Exact values live in the frontmatter; translucent structural colors are composited over their corresponding page backgrounds.

### Primary
- **Warm Brass:** the playing state, primary controls, active progress, and high-contrast focus ring.
- **Sea Glass:** connection, healthy secondary state, and selected supporting controls.

### Secondary
- **Coral Status:** errors, destructive actions, and offline conditions only.

### Neutral
- **Acoustic Ink:** dark ambient ground, intentionally softer than pure black.
- **Fog:** daylight ambient ground with a cool mineral cast.
- **Pearl:** primary text in Smokeglass.
- **Deep Ink:** primary text in Frostglass.
- **Smoke Pane / Frost Pane:** structural materials only.
- **Smoke Edge / Frost Edge:** structural boundaries and fine dividers.

### Named Rules

**The Music Makes Color Rule.** Chrome remains neutral; Warm Brass and Sea Glass appear because music is active or a connection has meaning.

**The Clear Edge Rule.** Structural boundaries use cool translucent white or ink. The dashboard brand hero alone may use a 1px Coral-to-Brass frame; colored borders elsewhere and luminous halos are not part of this material system.

## Typography

**Display Font:** Manrope (fallback: system-ui)
**Body Font:** Manrope (fallback: system-ui)
**Data Font:** ui-monospace (fallback: SFMono-Regular, Consolas)

**Character:** Manrope gives both titles and operational copy a contemporary human rhythm, varying weight and scale instead of switching personality. Data type is a precision tool, not a costume.

### Hierarchy
- **Display** (650, fluid 2.25–4.75rem, 0.98): current track title and singular listening moments.
- **Headline** (650, fluid 1.5–2.25rem, 1.12): page headings.
- **Title** (650, 1rem, 1.3): sections, panels, and row titles.
- **Body** (400, 0.9375rem, 1.55): descriptions and settings copy, capped around 70 characters where practical.
- **Label** (600, 0.75rem, 1.3): navigation, metadata labels, and compact controls in sentence case.
- **Data** (500, 0.75–0.875rem): IDs, durations, formats, byte counts, and measurements only.

### Named Rules

**The Human Voice Rule.** Navigation, headings, actions, and states use sentence case. Uppercase is reserved for real format abbreviations such as FLAC or API.

## Layout

Desktop uses three functional regions: a 248px navigation pane, a flexible content plane, and a 336px listening rail. The opening dashboard gives its largest area to current playback, with compact library health beneath and the queue continuously available in the rail. Administrative tools sit lower in navigation and open on the same clear content plane.

Below 1280px the listening rail docks beneath content as one horizontal acoustic pane. Below 1024px navigation becomes a drawer. At 760px the full rail gives way to a compact bottom player, content gains bottom clearance, forms stack, and tables use responsive horizontal containment without breaking the viewport.

The spacing system uses a 4px base with 8, 16, 24, 32, and 48px steps. Tight control groups use 8px; content groups use 16px; sections separate by 24–32px; major listening regions use 48px when space allows.

## Imagery

The official product icon is `michi-micro-server.svg`; render it in its original colors without theme filters, decorative containers, or cropping. Raster derivatives must preserve the full artwork and transparency.

The main dashboard hero presents the 2:1 source raster inside a cinematic 2.6:1–3:1 frame as a welcome and idle-state brand scene, not as routine navigation chrome. Desktop scales the raster by height so the cat occupies roughly 34–40% of the composition near the left quarter, while low-opacity equalizer bars, elliptical chest rings, and restrained Brass/Coral ambient light add audio depth. Real copy stays in one structural pane on the right. Mobile crops around the face and headphones, turns down the audio graphics, and reflows copy into a separate safe pane below so eyes and whiskers remain unobstructed.

Smokeglass keeps the image naturally dark with a single translucent content pane. Frostglass uses a denser mineral overlay only behind copy; it must preserve fur contrast rather than bleaching the whole image. Generated imagery must remain photorealistic, anatomically credible, intimate, musical, and free of violet, generic cyberpunk lighting, excessive neon halos, baked text, logos, UI, borders, and watermarks.

## Elevation & Depth

Structural glass uses `backdrop-filter: blur(22px) saturate(118%)` in Smokeglass and `blur(20px) saturate(112%)` in Frostglass. No content card, row, table, badge, or input may use backdrop filtering. The ambient color field is bounded behind the shell and uses soft radial fields rather than a full-page decorative wash.

### Shadow Vocabulary
- **Structural dark** (`0 18px 48px rgba(0, 0, 0, 0.32), 0 2px 8px rgba(0, 0, 0, 0.20)`): sidebar, listening rail, and floating structural panes.
- **Structural light** (`0 18px 44px rgba(56, 67, 78, 0.14), 0 2px 8px rgba(56, 67, 78, 0.08)`): Frostglass equivalents.
- **Modal** (`0 24px 64px rgba(0, 0, 0, 0.42), 0 4px 14px rgba(0, 0, 0, 0.24)`): protected focus surfaces.

**The Structural Glass Rule.** Blur belongs only to shell panes and transient overlays. Content depth comes from tonal contrast, fine borders, and artwork.

## Shapes

Controls use a confident 10px radius, content containers 14px, and structural glass 22px. Pills are reserved for status and compact segmented controls. Cover art uses 16–20px corners and remains the strongest solid shape in the listening scene. Borders stay 1px and cool-neutral.

## Components

### Buttons
- **Shape:** compact rounded controls (10px), at least 36px high.
- **Primary:** Warm Brass fill with Deep Ink text; reserved for playback and decisive creation actions.
- **Secondary / Ghost:** transparent or neutral tonal fill with a fine structural edge.
- **Hover / Focus:** subtle material lift on hover; a solid 2px Brass or Sea Glass focus outline with offset, never glow.

### Cards / Containers
- **Structural panes:** smoked or frosted translucent material, 22px corners, real offset shadow, and restrained top specular edge.
- **Content containers:** clear or tonal surface, 14px corners, 1px neutral border, no blur and usually no shadow.
- **Rows:** transparent by default, separated by mineral hairlines; hover uses a quiet neutral tint.

### Inputs / Fields
- **Style:** tonal translucent fill without blur, 10px corners, neutral edge, sentence-case placeholder.
- **Focus:** solid 2px Sea Glass or Brass outline with 2px offset.
- **Error:** Coral edge and supporting text, with no glow.

### Navigation
- Music destinations lead in the order Dashboard, Library, Playlists, Broadcast. Server and system destinations are separated lower down.
- Active navigation uses a quiet tonal fill, Pearl/Deep Ink text, and a small Brass material marker.
- Mobile navigation is a full-height Smokeglass/Frostglass drawer with an explicit backdrop.

### Now Playing

The wide-screen rail is one continuous structural pane containing cover, human-scale title and artist, progress, playback action, and Up Next. On track change, cover and title planes perform one soft 360ms clip-and-crossfade material transition; content is visible by default and all motion is removed under `prefers-reduced-motion`.

## Do's and Don'ts

### Do:
- **Do** let artwork and playback state provide color while structural chrome stays neutral.
- **Do** keep glass structural and content planes clear, tonal, and fast.
- **Do** preserve readable body type at 0.875rem or larger and labels at 0.75rem or larger.
- **Do** use offset plus soft blur for every elevated shadow.
- **Do** preserve the same information topology in Smokeglass and Frostglass.

### Don't:
- **Don't** turn rows, metrics, or settings groups into a wall of blurred cards.
- **Don't** use violet, neon halos, colored structural edges, or zero-offset glow shadows.
- **Don't** use all-uppercase or monospace styling as a visual costume.
- **Don't** place the cat in routine navigation or operational chrome.
- **Don't** hide core content before motion begins.
