# HTML & CSS Style

Builds on [general.md](./general.md).

## HTML
- Use semantic elements (`<header>`, `<nav>`, `<main>`, `<section>`, `<article>`, `<footer>`) over generic `<div>`s.
- Every form control has an associated `<label>`; images have meaningful `alt` text.
- Meet **accessibility** basics: logical heading order, sufficient color contrast, keyboard-navigable controls, ARIA only when semantics fall short.
- Lower-case tags/attributes; double-quote attribute values.

## CSS
- Keep specificity low: prefer classes over IDs and element selectors; avoid `!important`.
- Use a consistent naming convention (e.g. **BEM**) for class names.
- Centralize design tokens (colors, spacing, fonts) as **custom properties** (`--var`); don't hard-code magic values.
- Prefer **flexbox**/**grid** for layout; use relative units (`rem`, `%`, `fr`) over fixed pixels where it aids responsiveness.

## Responsiveness & performance
- Mobile-first; layer breakpoints with `min-width` media queries.
- Ensure touch targets are adequately sized (~44×44px) and text is readable without zoom.
- Minimize and bundle assets; optimize images.
