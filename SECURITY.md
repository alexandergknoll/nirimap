# Security Policy

## Supported versions

Only the latest release on the [releases page](https://github.com/alexandergknoll/nirimap/releases)
receives security fixes.

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/alexandergknoll/nirimap/security/advisories/new)
for this repository. Include the nirimap version (or commit), your Niri
version, and steps to reproduce.

You should get an acknowledgement within a week. Once a fix is available it
will be released and the advisory published with credit to the reporter
(unless you prefer otherwise).

## Scope

nirimap runs unprivileged as the logged-in user and talks only to the local
Niri compositor socket. Issues of particular interest:

- Crashes, hangs, or unbounded resource use triggered by another Wayland
  client (e.g. via window titles or `app_id` values).
- Problems in the release pipeline or published artifacts.

## Verifying release binaries

Release tarballs carry a signed build-provenance attestation. Verify one with
the GitHub CLI:

```bash
gh attestation verify nirimap-<tag>-x86_64-linux.tar.gz --owner alexandergknoll
```
