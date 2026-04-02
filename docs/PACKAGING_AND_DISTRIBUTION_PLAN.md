# PRISM Packaging And Distribution Plan

## Purpose

This document defines the execution plan for packaging and distributing PRISM as
one product with one user-facing front door:

- `prism ...` for normal CLI usage
- `prism mcp serve` for direct MCP serving
- `prism mcp start|stop|restart|status|health|logs` for managed local MCP
  daemon lifecycle

The plan is intentionally split into two phases:

1. ship the product cleanly for macOS and Linux
2. make the runtime and packaging model work equally well on Windows

The goal is not just to publish binaries. The goal is to make PRISM easy to
install, easy to invoke, and internally coherent so the packaging model matches
the product model.

## Current State

Today the repo is already close to the desired UX at the command level, but not
yet at the packaging/runtime level.

Observed product shape:

- `prism-cli` owns the user-facing CLI surface
- the CLI clap name is already `prism`
- `prism-cli` already exposes `mcp start|stop|restart|status|health|logs`
- `prism-mcp` owns the server/runtime surface and supports `stdio`, `daemon`,
  and `bridge` modes

Observed implementation split:

- the user-facing CLI is parsed in
  [crates/prism-cli/src/cli.rs](/Users/bene/code/prism/crates/prism-cli/src/cli.rs)
- the CLI entrypoint is a thin facade in
  [crates/prism-cli/src/main.rs](/Users/bene/code/prism/crates/prism-cli/src/main.rs)
- MCP lifecycle orchestration lives in
  [crates/prism-cli/src/mcp.rs](/Users/bene/code/prism/crates/prism-cli/src/mcp.rs)
- the MCP server entrypoint is a thin facade in
  [crates/prism-mcp/src/main.rs](/Users/bene/code/prism/crates/prism-mcp/src/main.rs)
- `PrismMcpCli` and `serve_with_mode` are already exposed from
  [crates/prism-mcp/src/lib.rs](/Users/bene/code/prism/crates/prism-mcp/src/lib.rs)

The main gaps are:

- the shipped package names are still `prism-cli` and `prism-mcp`, not one
  product named `prism`
- the CLI currently looks for a sibling `prism-mcp` binary instead of treating
  the MCP runtime as an internal execution mode
- daemon and process management are Unix-centric
- there is no release/distribution automation in the repo yet
- there is no current install story for end users on macOS, Linux, or Windows

## Product Decision

PRISM should ship as one application named `prism`.

That means:

- end-user docs should never require `prism-cli`
- end-user docs should never require `prism-mcp`
- MCP serving remains part of the product, but not as a separately installed
  user tool
- internal MCP runtime modes can remain real implementation concepts, but they
  should be launched through the `prism` executable

The intended public command model is:

```text
prism --help
prism entrypoints
prism symbol <name>
prism mcp serve
prism mcp serve --transport stdio
prism mcp start
prism mcp restart
prism mcp status
prism mcp health
```

The intended implementation model is:

- one physical user-facing binary: `prism`
- hidden internal subcommands or internal execution modes for daemon and bridge
  behavior
- self-reexec into those internal modes instead of spawning a separate visible
  `prism-mcp` binary

## Non-Goals

This plan does not require:

- publishing the internal workspace crates to crates.io as part of the initial
  install story
- removing stdio support from the MCP server
- introducing network-hosted PRISM as a product requirement
- supporting Windows in phase 1

## Release Strategy

The recommended release backbone is `cargo-dist` with GitHub Releases as the
canonical artifact host.

Why:

- it matches the Rust workspace and release model well
- it generates multi-platform release automation
- it supports installer generation for shell, PowerShell, Homebrew, npm, and
  MSI
- it is well-suited to one distributable application with prebuilt binaries

The installation story should be:

- macOS:
  - Homebrew tap
  - shell installer
  - direct GitHub Release tarball
- Linux:
  - shell installer
  - direct GitHub Release tarball
- Windows:
  - MSI
  - PowerShell installer
  - WinGet
  - direct GitHub Release zip

`cargo install` should not be the primary installation method. It can be added
later as an optional fallback if the crates.io publishing story becomes
desirable, but it should not define the main product experience.

## Cross-Cutting Constraints

The following constraints apply to both phases:

- preserve the existing facade rule: `main.rs` stays thin and crate roots remain
  facades
- keep the user-facing product named `prism`
- keep `mcp serve` and `mcp start` distinct:
  - `serve` is the direct foreground server path
  - `start` is the managed background daemon path
- preserve both MCP serving models:
  - direct foreground serving, typically stdio
  - long-lived local HTTP daemon with health/status/UI surfaces
- avoid a packaging model that allows the CLI and MCP runtime to drift to
  different installed versions

## Phase 1 Goal

Ship a coherent `prism` product for macOS and Linux with:

- one installed front-door executable
- direct MCP serving via `prism mcp serve`
- managed daemon lifecycle via `prism mcp start|stop|restart|status|health|logs`
- GitHub Release artifacts
- shell installers
- Homebrew distribution for macOS

Phase 1 is allowed to remain Unix-oriented internally as long as the user-facing
product is correct and polished on macOS and Linux.

## Phase 1 Workstreams

### 1. Unify The Product Around One Binary

Create one distributable binary named `prism`.

Recommended implementation:

- rename the distributable CLI package/binary surface from `prism-cli` to
  `prism`
- keep `prism-mcp` as a library crate if that remains the cleanest ownership
  boundary
- remove the end-user requirement that a separate `prism-mcp` binary be present
- introduce hidden internal execution paths so `prism` can re-exec itself for:
  - daemon mode
  - bridge mode
  - other MCP-internal lifecycle behavior if needed

Likely touch points:

- [Cargo.toml](/Users/bene/code/prism/Cargo.toml)
- [crates/prism-cli/Cargo.toml](/Users/bene/code/prism/crates/prism-cli/Cargo.toml)
- [crates/prism-cli/src/cli.rs](/Users/bene/code/prism/crates/prism-cli/src/cli.rs)
- [crates/prism-cli/src/commands.rs](/Users/bene/code/prism/crates/prism-cli/src/commands.rs)
- [crates/prism-cli/src/mcp.rs](/Users/bene/code/prism/crates/prism-cli/src/mcp.rs)
- [crates/prism-mcp/src/lib.rs](/Users/bene/code/prism/crates/prism-mcp/src/lib.rs)

Acceptance criteria:

- a user installs one executable named `prism`
- `prism --help` shows the product help
- `prism mcp start` works without any sibling `prism-mcp` binary on disk
- daemon startup and bridge execution are launched through `prism`

### 2. Normalize The Public MCP Command Surface

Make the command surface explicit and stable.

Required public shape:

- `prism mcp bridge`
- `prism mcp serve`
- `prism mcp start`
- `prism mcp stop`
- `prism mcp restart`
- `prism mcp status`
- `prism mcp health`
- `prism mcp logs`

Recommended details:

- `prism mcp bridge` should be the canonical editor/host integration entrypoint
- `prism mcp bridge` should auto-resolve the workspace root from the launch cwd
  when `--root` is omitted, then spawn or reuse that workspace's managed daemon
- `prism mcp serve` should be the canonical direct foreground command
- `prism mcp serve` should default to stdio unless there is a strong reason to
  prefer an explicit transport flag
- if transport selection is exposed, use a stable option such as
  `--transport stdio|http`
- `prism mcp start` remains the managed local HTTP daemon entrypoint

Acceptance criteria:

- the docs can explain the distinction between `serve` and `start` in one short
  paragraph
- editor integrations can use one static command entry such as
  `prism mcp bridge` without hard-coding a checkout-specific root
- the public MCP command set no longer depends on users understanding the
  internal `prism-mcp` executable

### 3. Remove Sibling-Binary Lookup

Replace the current `prism_mcp_binary()` lookup pattern with self-reexec or a
similarly unified internal launch model.

Current problem:

- the CLI explicitly searches for a sibling `prism-mcp` binary
- this makes installs more fragile
- this creates version-skew risk
- this ties packaging to the current split implementation

Required outcome:

- lifecycle commands launch internal MCP runtime modes through the installed
  `prism` binary
- packaging no longer depends on colocating two user-visible executables

Acceptance criteria:

- the installed artifact set for macOS/Linux can contain only `prism`
- lifecycle commands do not fail because a sibling `prism-mcp` executable is
  missing

### 4. Add Release Metadata And Dist Config

Add the metadata needed for distribution.

Required metadata:

- repository URL
- package description
- readme
- authors
- homepage or documentation URL if applicable
- license and license files if dist packaging needs explicit paths

Add a `cargo-dist` configuration with:

- GitHub CI
- release artifacts for macOS and Linux
- shell installers
- Homebrew publication

The phase 1 target list should include at least:

- `x86_64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

The exact target set can be expanded later if Linux musl or additional variants
are desired.

Likely touch points:

- [Cargo.toml](/Users/bene/code/prism/Cargo.toml)
- optional `dist-workspace.toml`
- generated `.github/workflows/release.yml`
- release documentation and changelog surfaces if needed

Acceptance criteria:

- the repo can generate release automation from `cargo-dist`
- a tag-driven release path exists
- macOS and Linux artifacts are uploaded to GitHub Releases

### 5. Ship Homebrew And Shell Installers

Provide the easiest install stories first for macOS and Linux.

macOS:

- publish a Homebrew formula to a dedicated tap
- keep install command to one line
- document Homebrew as the preferred macOS path

Linux:

- publish a shell installer that downloads the right release artifact
- document tarball install as the fallback/manual path

Recommended user-facing examples:

```text
brew install <org>/homebrew-tap/prism
curl -fsSL <install-url> | sh
```

Acceptance criteria:

- a macOS user can install PRISM with one Homebrew command
- a Linux user can install PRISM with one shell command
- both install paths yield the `prism` executable on `PATH`

### 6. Harden Release Outputs

Make the release outputs credible and supportable.

Required outputs:

- per-target archives
- checksums
- release notes

Strongly recommended:

- GitHub artifact attestations
- signing where practical for shipped artifacts

This is not phase 1 blocking if it delays the initial install story too much,
but it should be included in the release pipeline plan rather than treated as an
afterthought.

Acceptance criteria:

- each release has checksums and structured artifacts
- the release pipeline is ready for artifact verification and later signing

## Phase 1 Risks

- renaming the binary/package surface can ripple into docs and release tooling
- self-reexec work can leak internal command semantics into public help if not
  carefully hidden
- Homebrew publication adds an additional repository and credential surface
- shell-installer success depends on good install-path defaults and clear
  messaging

## Phase 1 Done When

Phase 1 is complete when:

- PRISM ships as one installed executable named `prism`
- `prism mcp serve` and `prism mcp start` are both supported and clearly
  differentiated
- macOS and Linux users have easy install paths
- GitHub Releases are the canonical artifact host
- the packaging model no longer depends on a separately installed
  user-visible `prism-mcp` binary

## Phase 2 Goal

Make PRISM work equally well on Windows, both at runtime and in installation.

Phase 2 is not just “cross-compile and zip the binary.” It must remove the
remaining Unix-only lifecycle assumptions from the CLI and MCP runtime.

## Phase 2 Workstreams

### 1. Replace Unix-Only Process Management

Current blockers:

- daemon spawning in
  [crates/prism-mcp/src/daemon_mode.rs](/Users/bene/code/prism/crates/prism-mcp/src/daemon_mode.rs)
  uses `/bin/sh` and `nohup`
- process inspection in
  [crates/prism-cli/src/mcp.rs](/Users/bene/code/prism/crates/prism-cli/src/mcp.rs)
  uses `ps`
- port-owner inspection uses `lsof`
- process termination uses POSIX signal semantics
- daemonization in
  [crates/prism-mcp/src/process_lifecycle.rs](/Users/bene/code/prism/crates/prism-mcp/src/process_lifecycle.rs)
  is Unix-only
- runtime liveness checks in
  [crates/prism-mcp/src/runtime_state.rs](/Users/bene/code/prism/crates/prism-mcp/src/runtime_state.rs)
  use `libc::kill`

Required direction:

- create a platform abstraction for MCP process lifecycle management
- move platform-specific process behavior into focused modules
- keep lifecycle decisions driven by PRISM runtime state plus health checks,
  not by ad hoc shell command parsing

Recommended decomposition:

- `platform/process.rs`
- `platform/process_unix.rs`
- `platform/process_windows.rs`
- `platform/ports.rs`
- `platform/ports_unix.rs`
- `platform/ports_windows.rs`

Acceptance criteria:

- Windows startup, stop, restart, and status do not depend on Unix tools
- the CLI can manage the daemon on Windows without shelling out to `/bin/sh`,
  `ps`, or `lsof`

### 2. Make Internal Launch And Daemonization Portable

The phase 1 self-reexec model must be extended to Windows.

Required outcome:

- `prism mcp start` can detach or background the daemon appropriately on Windows
- `prism mcp stop` can stop it cleanly
- `prism mcp restart` can reclaim the expected port and recover from stale state

Important design rule:

- do not preserve Unix daemonization mechanics as the product contract
- preserve the user contract and re-implement the internal mechanics per
  platform

Acceptance criteria:

- Windows lifecycle behavior matches the macOS/Linux user experience closely
- the same command set works on all three operating systems

### 3. Add Windows Packaging Outputs

Publish first-class Windows installers.

Required outputs:

- `.msi`
- PowerShell installer
- GitHub Release zip

Recommended user-facing install paths:

```text
winget install <publisher>.prism
powershell -ExecutionPolicy Bypass -c "irm <install-url> | iex"
```

The MSI should be the canonical Windows installer because it fits enterprise and
tooling expectations better than a raw zip.

Acceptance criteria:

- Windows users have a one-command install path
- the installed result is the `prism` executable on `PATH`

### 4. Add WinGet Publication

Phase 2 should include WinGet publication as the preferred Windows package
manager path.

Recommended implementation:

- keep GitHub Releases as the canonical artifact host
- publish MSI artifacts there
- add a release job that creates or updates the WinGet manifest referencing the
  MSI

This is likely a custom publish job rather than native `cargo-dist`
functionality.

Acceptance criteria:

- stable releases are available through WinGet
- the WinGet manifest points to the MSI artifacts from the canonical release

### 5. Validate Path, Process, And Runtime Assumptions

Phase 2 must audit all assumptions that are commonly invisible on Unix:

- path separator handling
- quoting and argument escaping
- executable suffix handling
- install-location defaults
- config and state file locations
- detached-process behavior
- port and process inspection

Acceptance criteria:

- no user-facing command requires Unix shell semantics on Windows
- lifecycle and install docs do not need platform apology text

## Phase 2 Risks

- Windows lifecycle behavior is the highest technical-risk part of this plan
- mixing shell-based lifecycle logic with Windows support will produce fragile
  behavior and should be avoided
- MSI and WinGet publication add packaging and release-management complexity
- background-process semantics differ enough that “small conditional patches”
  may be worse than a proper platform boundary

## Phase 2 Done When

Phase 2 is complete when:

- `prism` installs cleanly on Windows
- `prism mcp serve` and `prism mcp start|stop|restart|status|health|logs` work
  on Windows
- Windows no longer depends on Unix lifecycle internals
- MSI, PowerShell installer, GitHub Release zip, and WinGet are all supported

## Recommended Execution Order

Within the two phases, the recommended order is:

1. unify the product around one `prism` binary
2. normalize the public MCP command surface
3. replace sibling-binary lookup with self-reexec
4. add dist config and release automation
5. ship macOS/Linux installers and GitHub Releases
6. extract platform-specific lifecycle abstractions
7. implement Windows lifecycle support
8. add MSI, PowerShell installer, and WinGet publication

## Decision Log

The following product decisions are intentional and should not be re-litigated
during execution unless new evidence appears:

- PRISM ships as one product named `prism`
- `prism mcp serve` remains part of the public product surface
- the HTTP daemon remains part of the product surface
- stdio serving is retained as a supported direct server mode
- end users should not need to know about `prism-cli` or `prism-mcp`
- macOS/Linux ship first
- Windows parity is phase 2, not a blocker for phase 1

## Suggested Follow-Up Documents

Once implementation starts, the following follow-up artifacts would be useful:

- a short release-operator runbook
- a Homebrew tap maintenance note
- a Windows packaging note covering MSI and WinGet publication
- updated user-facing installation instructions in the main README
