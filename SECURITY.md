# Security Policy

## Supported Versions

Security fixes are provided for the latest released version of `enzo`.

If possible, test suspected vulnerabilities against the newest release before
reporting. If you can only reproduce the issue on an older version or a
development commit, include that version or commit in the report.

## Reporting a Vulnerability

Please do not open a public GitHub issue for suspected security
vulnerabilities.

Report vulnerabilities through GitHub private vulnerability reporting for this
repository:

<https://github.com/MiguelRegueiro/enzo/security/advisories/new>

When reporting, please include:

- the affected `enzo` version or commit
- your operating system, terminal, and whether tmux is involved
- relevant FFmpeg, PulseAudio, or other native dependency versions, when known
- a clear description of the issue and its potential impact
- reproduction steps or a proof of concept, when possible
- the media or subtitle format and whether the input is a local file or URL
- a reduced sample file when one is necessary and safe to share

Reports will be reviewed and triaged as promptly as practical. Confirmed
vulnerabilities will be fixed and disclosed with appropriate release notes once
a fix is available. Please avoid public disclosure until a fix or a coordinated
disclosure plan is ready.

## Project Security Scope

`enzo` is a local terminal video player. It does not run a server or accept
remote logins, but it processes user-selected local files and URLs through
native media libraries and renders output through terminal graphics protocols.

Security-sensitive areas include:

- malformed or malicious media and subtitle files processed by FFmpeg and
  Enzo's native C code
- memory safety across Rust, C, and FFmpeg FFI boundaries
- frame-buffer sizing, decoding, scaling, and subtitle bitmap handling
- remote URL and media protocol handling
- Kitty graphics escape sequences and tmux passthrough
- shared-memory frame creation, permissions, cleanup, and naming
- resume-state paths, fingerprints, file permissions, and record parsing

Issues that exist entirely in an upstream dependency, terminal emulator, or
operating system should normally be reported upstream. If you are unsure
whether Enzo contributes to the issue, report it privately here first.

## Remote Media Policy

Enzo accepts HTTP and HTTPS media URLs. Remote playlists may follow HTTP(S)
references across hosts and use FFmpeg's `data` and HTTP(S) `crypto` wrappers,
but they cannot open local files or arbitrary FFmpeg protocols. All media I/O
is read-only and each blocking FFmpeg operation has a 60-second deadline.

## Dependency Advisories

Rust and native dependency advisories are reviewed during maintenance and
release work. If your report concerns an advisory, include its CVE or GHSA
identifier, affected versions, and any known impact on `enzo`.
