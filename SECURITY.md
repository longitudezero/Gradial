# Security policy

## Supported versions

Gradial is currently in public beta. Security fixes are made against the newest
published beta only.

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Use GitHub's
private **Report a vulnerability** form on this repository's Security page.
Include reproduction steps, affected version, impact, and any suggested
mitigation. Do not include real credentials, private files, or unrelated user
data.

You should receive an initial response within seven days. Public disclosure
will be coordinated after a fix or mitigation is available.

## Plugin trust boundary

Gradial's Lua restrictions and capability approvals are defense in depth.
Version 0.3 does not claim that hostile downloaded plugin source is safe.
Users must inspect or otherwise trust a local plugin before enabling it. See
[Plugin trust and capabilities](Documentation/plugin-trust-and-capabilities-guide.md).
