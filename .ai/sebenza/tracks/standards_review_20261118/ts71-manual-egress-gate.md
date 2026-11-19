# TS-71 manual gate: enabling a review CLI without an egress probe

TS-71 is a manual (M) scenario. A CLI is dispatched standards-review runs only when its row in
`conformance::REVIEW_EGRESS` (`crates/sebenza-server/src/services/role_runner/mod.rs`) is marked
egress-verified. This checklist decides whether a CLI with no automated probe may be marked so.
Nobody has signed it yet: it has not been carried out.

CLI and version: ____________________  Date: ____________  Operator: ____________________

## Read-only

- [ ] The CLI is `Verified` in `conformance::TABLE`: its read-only conformance test (TS-41) passed
      against this version.
- [ ] Run against a read-only head export, it neither writes nor deletes a file. Before and after
      fingerprints of the export match.

## No network tools

- [ ] Every tool that can fetch a URL or search the web is disabled. List each flag used:
      ____________________
- [ ] No MCP server, plugin or extension loads, from user or project settings.
- [ ] Hooks and other user-configured commands are disabled.

## Process egress

- [ ] The CLI honours `HTTPS_PROXY`/`HTTP_PROXY`. With the daemon's egress proxy set, a request to
      a non-allowlisted host is refused (403) and the CLI does not retry around the proxy.
- [ ] Its model endpoint is reachable only through the proxy, as `host:port`: ____________________
- [ ] Nothing else is contacted during a run, such as telemetry or update checks, or each such
      endpoint is listed and accepted here: ____________________
- [ ] Residual accepted: a process that ignores the proxy variables is not blocked by the proxy.
      A host firewall is in place: yes / no (circle one).

## Credentials

- [ ] The run token is read only from the environment and never echoed to stdout, stderr or files.
- [ ] The CLI cannot read host credentials outside the export: `~/.ssh`, `~/.aws`, `~/.config` and
      Sebenza's `~/.ai`.

## Decision

- [ ] All boxes above are ticked, so the CLI's `REVIEW_EGRESS` row may be set to `true` in a
      reviewed commit that cites this checklist.
- [ ] Not all boxes are ticked, so the CLI stays disabled and its jobs stay queued
      (`cli-not-conformant`).

Signature: ____________________
