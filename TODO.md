# TODO

## Conductor Tracks — make editable from the frontend (next phase)
The Tracks (Kanban) view is currently **read-only**. Next phase: allow editing a
track's `spec.md`, `design.md`, and `plan.json` directly in the Tracks view.

- Add an edit toggle to the Plan/Spec/Design tabs in `ConductorTrackDetail` (a
  textarea for the markdown/JSON; validate `plan.json` parses before save).
- New backend write endpoint `PUT /{prefix}/api/worktrees/{name}/conductor-file`
  (body `{ path, content }`) that writes back through the same conductor-dir
  resolution + path-traversal guard used by the read path
  (`crates/common/src/adapters/fs.rs`).
- Add the route to the ts-rest contract (`frontend/src/lib/api-contract`) + an
  `api.ts` wrapper, and refresh the board after a successful save.
