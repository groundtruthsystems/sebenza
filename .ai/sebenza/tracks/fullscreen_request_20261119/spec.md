# Spec — Full-screen request view (bug)

Track: `fullscreen_request_20261119` · Type: bug · Source: inbox item "sebenza updates", handover Track A (§2.7), requirements AC-07, FR-12, FR-13, NFR-04. No design stage, as the handover allows for bugs.

## Overview
Inbox requests, and the documents and notes inside one (the brief, the requirements, each note), cannot be opened full screen, and the handover full screen is local state the URL does not carry. This track adds a URL-addressable full-screen view: `view=full` added to the request's own route. Leaving it returns the user to where they were, with the same selection and scroll position.

## Functional Requirements
- FR-01: `/inbox?item=<id>&view=full` opens that request full screen, using the existing fetch and access checks.
- FR-02: `&doc=<key>` opens one document or note of the request full screen (brief, requirements, a note id).
- FR-03: An unknown or inaccessible id in full screen shows the existing not-found state, never a blank page.
- FR-04: Entry from the list or the detail pushes a history entry whose state holds the originating route, selection and scroll.
- FR-05: Close, Esc and browser Back after in-app entry restore the originating route, selection and scroll.
- FR-06: After a direct URL or reload, Close and Esc go to `/inbox?item=<id>` with that request selected and scrolled into view.
- FR-07: After a direct URL or reload, Back follows browser history and does not trap the user.
- FR-08: Handover full screen moves to the same `view=full` URL pattern (`&handover=<slug>`) and keeps its Esc stack, so nested overlays close innermost first.
- FR-09: Track views gain no full screen.

## Non-Functional Requirements
- NFR-01: No new dependencies; routing stays on the History API (no router library).
- NFR-02: Keyboard accessible: the full-screen view has a labelled Close control, and focus returns to the control that opened it.

## Acceptance Criteria
- AC-01 (FR-01, FR-04, FR-05; note "Acceptance Criteria" item 7): entry from the list opens full screen and Close/Esc/Back restore list context. TS-01, TS-02, TS-12
- AC-02 (FR-02, FR-04, FR-05): entry from the detail for a request and for a document or note restores the detail context. TS-03, TS-04
- AC-03 (FR-06, FR-07): direct URL and reload in full screen behave per FR-06/FR-07. TS-05, TS-06, TS-12
- AC-04 (FR-03): an unknown id shows not-found. TS-07
- AC-05 (FR-08): handover full screen works via Close, Esc and Back with nested overlays. TS-08, TS-09
- AC-06 (FR-09, NFR-01, NFR-02): no track-view full screen; focus returns; no new dependency. TS-10, TS-11

## Out of Scope
- Full screen for track views.
- Server changes: existing fetch and access checks are reused unchanged.

## Decisions on test-plan questions
- Back with no prior in-app history entry follows the browser's default (it may leave the app); Close/Esc are the in-app exits.
- An unknown `doc=` key on a valid request shows the not-found state for that document, with the request still reachable via Close.
- Existing handover full-screen buttons push the `view=full` URL instead of setting local state; no separate legacy path remains.
