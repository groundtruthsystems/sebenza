# Test Plan: Full-screen request view (bug)

Track: `fullscreen_request_20261119` · Spec: `spec.md` (AC-01..AC-06, FR-01..FR-09, NFR-01..02). No design stage; traceability is keyed on acceptance criteria.

## Strategy
This is a frontend routing and state bug. Nearly all behaviour (URL parsing, History API pushes, popstate restore, Esc stack, focus return) is provable in vitest with Testing Library under jsdom, colocated in `InboxView.test.tsx`, `HandoverReview.test.tsx` and `escStack.test.tsx`. Tests drive the History API directly (`history.pushState`, `window.history.back()`, dispatching `popstate`) and mock the existing fetch layer; no server is involved. jsdom has no layout, so scroll restore is asserted only as "the stored scroll value is written and re-applied via a spied `scrollTo`/`scrollTop` setter". Real scroll behaviour gets one manual browser check (TS-12). Deliberately not tested: server access checks (unchanged, out of scope), track-view layout, performance, E2E automation (no tooling exists).

## Test Levels
| Level | Scope | Tooling | Where it runs |
|---|---|---|---|
| Component/integration | InboxView and HandoverReview with real history state, mocked fetch | vitest + Testing Library, jsdom | `cd frontend && CI=true npm test`; CI |
| Unit | escStack ordering, route/state parse helpers if extracted | vitest | same |
| Static check | No new dependency (package.json diff) | `git diff` / review | Review gate |
| Manual | Scroll and Back in a real browser | Chrome, dev build | Developer, once before close |

## Scenarios
| TS | Covers | Level | Priority | Given / When / Then | Evidence |
|---|---|---|---|---|---|
| TS-01 | AC-01 (FR-01, FR-04, FR-05) | Component | P1 | Given list with item B selected, scrolled; when full-screen opened then Close clicked; then route, selection, scroll restored. | Assert pushState called with state {route,selection,scroll}; URL `/inbox?item=B`; B selected; scroll spy called with saved value |
| TS-02 | AC-01 (FR-04, FR-05) | Component | P1 | Given full screen opened from list; when Esc pressed, and separately Back fired; then list context restored both ways. | Same assertions as TS-01 after Esc keydown and after `history.back()` + popstate; history length unchanged by Close |
| TS-03 | AC-02 (FR-01, FR-04, FR-05) | Component | P1 | Given request detail open; when full screen entered then Close/Esc/Back; then detail route and selection restored. | URL equals originating detail URL; detail pane rendered for same item |
| TS-04 | AC-02 (FR-02, FR-04, FR-05) | Component | P1 | Given detail open; when brief, requirements and a note opened via `&doc=`; then each shows that document and Close restores detail. | Rendered heading/body matches doc key (brief, requirements, note id); URL contains `doc=<key>`; restore as TS-03 |
| TS-05 | AC-03 (FR-06) | Component | P1 | Given page loaded directly at `/inbox?item=A&view=full` (history length 1); when Close or Esc; then `/inbox?item=A`, A selected and scrolled into view. | URL assertion; A has selected state; `scrollIntoView` spy called on A |
| TS-06 | AC-03 (FR-07) | Component | P1 | Given direct URL or reload into full screen with a prior history entry; when Back; then browser history is followed, no re-entry trap. | After `history.back()` URL is the prior entry, not `view=full`; no pushState on popstate; second Back leaves too |
| TS-07 | AC-04 (FR-03) | Component | P1 | Given fetch returns 404, and variant 403, and unknown `doc=` key; when `view=full` opened; then existing not-found state shown. | Not-found element present; main content not empty; no thrown error |
| TS-08 | AC-05 (FR-08) | Component | P1 | Given handover review; when full screen entered; then URL is `view=full&handover=<slug>`, and Close, Esc, Back each exit it. | URL pattern assert; local-only state gone (reload of URL re-opens handover); three exits restore prior route |
| TS-09 | AC-05 (FR-08) | Unit + component | P1 | Given nested overlay open inside handover full screen; when Esc pressed; then innermost closes first, full screen second. | escStack order assert: first Esc leaves full screen mounted, second unmounts it |
| TS-10 | AC-06 (FR-09) | Component | P2 | Given a track view; when `view=full` appended or UI inspected; then no full-screen control or view renders. | No full-screen button in TrackDetail; route with `view=full` on tracks renders normal view |
| TS-11 | AC-06 (NFR-01, NFR-02) | Component + static | P2 | Given full screen opened by keyboard from a control; when closed; then focus returns to that control; Close has an accessible name; package.json unchanged. | `toHaveFocus()` on opener; `getByRole('button',{name:/close/i})`; `git diff` shows no dependency change |
| TS-12 | AC-01, AC-03 (FR-05, FR-06) | Manual | P2 | Given a long list in Chrome, scrolled; when open full screen, Close, then reload and Close; then scroll and selection are correct. | Tester checklist ticked in the track plan; note of browser version |

TS-12 is added: jsdom cannot prove real scroll positions, so FR-05/FR-06 scroll restore had no real-browser evidence. All other IDs and mappings are unchanged from the spec.

## Traceability
| AC | Scenarios |
|---|---|
| AC-01 | TS-01, TS-02, TS-12 |
| AC-02 | TS-03, TS-04 |
| AC-03 | TS-05, TS-06, TS-12 |
| AC-04 | TS-07 |
| AC-05 | TS-08, TS-09 |
| AC-06 | TS-10, TS-11 |

FR coverage: FR-01 TS-01/03; FR-02 TS-04; FR-03 TS-07; FR-04 TS-01..04; FR-05 TS-01..04,12; FR-06 TS-05,12; FR-07 TS-06; FR-08 TS-08/09; FR-09 TS-10; NFR-01/02 TS-11. No orphan scenarios.

## Test Data & Environments
- Synthetic inbox items only (ids like `req-a`, `req-b`, titles "Sample request"); no real PHI/PII or credentials.
- Fetch mocked per test with fixtures for 200, 403 and 404 responses.
- A list of 30+ synthetic items for scroll and selection cases.
- Fixture request with brief, requirements and two notes for `doc=` cases.
- Handover fixture with a slug and one nested overlay (reuse existing HandoverReview test setup).
- jsdom: stub `scrollTo`, `scrollIntoView` and scroll setters; reset `history` and `location` in `beforeEach`.
- Manual check runs against a local dev build with the synthetic seed data.

## Entry & Exit Criteria
- Entry: spec approved; failing tests for TS-01..TS-11 written first (Red) per workflow.
- Entry: existing frontend suite is green before changes.
- Exit: TS-01..TS-11 pass under `CI=true npm test`.
- Exit: `npm run test:coverage` shows the workflow bar of >80% on changed modules.
- Exit: TS-12 manual checklist completed and recorded.
- Exit: no new dependency in `frontend/package.json` or lockfile.
- Exit: existing InboxView, HandoverReview and escStack tests still pass unmodified, except where handover state moved to URL.
- Exit: typecheck and lint clean.

## Coverage Map
Not produced: the AC-to-TS table is a near one-to-one mapping and already shows the fan-out.

## Open Questions
- TS-06: should Back after reload with no prior entry (history length 1) leave the app or be a no-op? Assumed browser default; confirm.
- TS-07: is a doc key unknown to a valid request treated as not-found or as falling back to the request? Plan assumes not-found.
- TS-08: should legacy handover local-state entry points still work with no URL? Assumed no; one path only.
