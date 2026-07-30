# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| 1.0.x   | :white_check_mark: |
| < 1.0   | :x: |

Security fixes target the latest release line — there's no long-term support for older versions from a
solo-maintained project.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Instead, report it privately via [GitHub Security Advisories](https://github.com/TomCahill/PushGit/security/advisories/new)
for this repository. This lets a fix land before the report becomes public.

Include as much detail as you can:

- A description of the vulnerability and its potential impact
- Steps to reproduce, or a proof-of-concept
- Affected version/commit
- Any suggested mitigation, if you have one

### What to expect

This is a small, solo-maintained project — there's no formal SLA or bug bounty. You'll get a genuine
best-effort response and credit in the advisory/release notes unless you'd prefer to stay anonymous.

## Scope

PushGit opens git repositories it did not create and treats their contents (working tree, `.git`
directory, hooks, filters, submodule config) as untrusted input. Reports involving hook execution,
path traversal, or other issues triggered by opening a malicious repository are in scope. Reports
about a repository's own contents (e.g. secrets committed by a user) are not.
