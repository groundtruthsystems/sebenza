# LXC / Apple worktree sandboxes — Spec

## Overview

When a worktree is created with `runtime: lxc` (Linux) or `runtime: apple` (macOS 26 Apple Silicon), Sebenza starts a long-lived sandbox, mounts the worktree at the same absolute host path, and runs agent/shell panes inside it as the host user. Host "Open in…" launchers are unchanged. Docker is no longer launchable.

## Functional Requirements

### FR1 — Runtime kinds

1. `RuntimeKind` includes `host`, `docker`, `lxc`, and `apple`.
2. YAML `runtime: lxc` and `runtime: apple` deserialize on `ProfileConfig`.
3. YAML `runtime: docker` still deserializes. A parse failure must not drop the rest of `profiles:`.
4. `runtime_kind_str` maps `Lxc → "lxc"`, `Apple → "apple"`, existing values unchanged.
5. `WorktreeMeta.runtime` is stored as those strings.

### FR2 — Profile validation

1. `runtime: lxc` or `apple` without `image` is 422 ("image is required").
2. `runtime: lxc` off Linux is 422 (LXC is Linux-only).
3. `runtime: apple` unless the host is macOS 26+ Apple Silicon is 422.
4. `runtime: docker` on create, open, or adopt is 422 naming the replacement (`lxc` on Linux, `apple` on macOS).
5. `runtime: host` is unchanged.

### FR3 — LXC adapter (Linux)

1. Detect classic LXC via `lxc-create` on PATH. If only LXD's `lxc` exists, error tells the user to `apt install lxc`.
2. Preflight: user subuid/subgid range and `lxc-usernet` veth allowance. Fail with the exact fix.
3. Name: `sebenza-<sanitised-branch>-<millis>` (same sanitiser rules as today's Docker names).
4. Unprivileged idmap includes a 1:1 hole for the host uid and gid. Do not chown the worktree.
5. Same-path bind mounts: worktree RW, `{repo}/.git` RW, repo RO, plus existing credential dirs at the same host paths when present. Profile `mounts:` use the same `MountSpec` (default guest path = host path; default read-only).
6. Env: `IS_SANDBOX=1`, `HOME` = host home, `TERM`, git `safe.directory` for wt + repo, env passthrough + runtime env (same reserved-key list as today's Docker builder).
7. Network: veth on `lxcbr0`; publish each allocated service port on `127.0.0.1` via a host-side proxy to the container IP.
8. Launch reuses a running instance with the branch prefix; starts a stopped one; does not create a second.
9. Remove: `lxc-stop -k` + `lxc-destroy -f` for every matching name.
10. `image: ubuntu:24.04` (and `ubuntu/noble`) parses to the download template (`-d ubuntu -r noble -a <host arch>`).

### FR4 — Apple adapter (macOS)

1. Detect `container` on PATH and a healthy `container system` (else tell the user to run `container system start`).
2. Same naming scheme as LXC.
3. Create a **machine** (not `container run`) with `--home-mount none` and explicit same-path shares for worktree, repo, `.git`, and credential dirs. If extra shares are impossible and all paths are under `$HOME`, fall back to `home-mount=rw` and document it.
4. Do not chown the worktree. Guest user is the host username/UID.
5. Same env contract as FR3.6.
6. Reuse running / start stopped / delete `--force` on remove.
7. `image:` is an OCI ref passed to `container machine create`.

### FR5 — Pane commands

1. LXC panes: `lxc-attach` as host uid/gid, cwd = worktree path, source `runtime.env`, then agent or interactive shell.
2. Apple panes: `container exec --uid --gid --workdir` (or `container machine run` if exec is not wired for machines — one path only).
3. PATH fallback uses host-home tool dirs (`$HOME/.local/bin`, `$HOME/.opencode/bin`, `$HOME/.grok/bin`, cargo/bun), not `/root/...`.
4. Host runtime pane commands are unchanged.

### FR6 — Lifecycle

1. Create/open materializes the sandbox then the tmux session.
2. Close kills tmux only; the instance remains.
3. Remove/merge destroys the instance then the git worktree.
4. Failed create rolls back the git worktree **and** the instance.
5. Tabs: refuse create/fork on `lxc`/`apple` (409), same wording family as today's Docker refusal. Restore is a no-op.
6. If `meta.runtime == "docker"`, remove still best-effort deletes matching `sebenza-<branch>-*` docker containers. Launch never uses Docker.

### FR7 — Init, CLI, docs

1. `sebenza-cli init` optional tools: `lxc-create` on Linux, `container` on macOS. Drop `docker`.
2. Commented example `sandbox` profile uses `lxc` or `apple` for the current OS. Default profile stays `host`.
3. CLI remains `add --profile <name>`. No `--lxc` / `--apple` flags.
4. README, `.ai/sebenza.example.yaml`, `tech-stack.md`, `workflow.md`, and `init_authoring` describe LXC/Apple, not Docker, as the sandbox.

### FR8 — Host editors

1. Launchers stay host-side against `${WORKTREE_PATH}`.
2. A file written by the agent is owned by the host uid/gid so Zed/IntelliJ can save it.

## Non-Functional Requirements

1. Unit tests mock CLIs. `cargo test` does not require LXC or Apple Container.
2. Existing YAML without `lxc`/`apple` keeps loading.
3. Errors state what failed and what to install or configure next.
4. No privileged LXC. Fail closed if idmap cannot be applied.
5. Service proxies bind loopback only.

## Acceptance Criteria

- [ ] Creating a worktree with `runtime: lxc` on Linux starts an unprivileged container; `pwd` in the agent pane equals the host worktree path.
- [ ] Creating a worktree with `runtime: apple` on macOS 26 Apple Silicon starts a container machine with the same path identity.
- [ ] A file written in the pane is host-uid owned; "Open in…" can open and save it.
- [ ] Close leaves the instance; remove/merge deletes it.
- [ ] `runtime: docker` YAML loads; create/open/adopt are 422; remove cleans leftover containers if Docker is present.
- [ ] Host profiles are unchanged.
- [ ] Wrong OS or missing tool is 422, not a hang.
- [ ] `cargo test` passes without a hypervisor.

## Out of Scope

- Incus / LXD.
- Multipass.
- Always-on sandboxing.
- Privileged LXC, nesting, Docker-in-LXC.
- Tabs inside sandboxes.
- A Sebenza-published base image.
- Intel Mac / macOS &lt; 26 Apple Container.
- Launchers that exec into the sandbox.
