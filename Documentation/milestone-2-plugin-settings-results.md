# Milestone 2 plugin settings results

Date: 2026-08-10

## Outcome

Gradial now has a Plugins page backed by immutable snapshots from the dedicated
plugin worker. It lists discovered and remembered packages with name, ID,
version, plugin API, requested permissions, effective status, and safe problem
messages. The page can rescan packages, enable, disable, reload, and open the
managed plugins folder without blocking the renderer.

Plugin operations are machine-local and immediate. They do not make the
configuration draft dirty, do not wait for **Save changes**, and are not
included in configuration import or export.

## Trust and admission

The first enable request and every changed permission set require a visible
confirmation that names the package, explains each permission, and warns that
Lua plugins should be enabled only when the user trusts their source. Approval
and enablement remain separate serialized worker transactions. After approval,
Gradial repeats discovery and all admission validation before loading code.

The worker now independently enforces that boundary as well: an enable command
checks approval before constructing the Lua adapter. A caller cannot bypass the
Settings confirmation path and cause unapproved source to be evaluated. Tests
cover this with a package whose entry source would fail immediately if run.

## Rescan and status

**Rescan plugins** performs discovery, package validation, trust
reconciliation, and inventory refresh, but executes no Lua. If an enabled
package changed or lost authorization, its existing adapter is unloaded and
the page reports that reload or permission review is required. Source edits
retain unchanged permission approval but never become active until explicit
reload.

Only safe stable error codes cross the worker boundary. Settings maps those to
plain-language messages and never displays Lua error payloads, source text, or
arbitrary package paths. Invalid packages remain isolated inventory entries so
one broken directory cannot hide healthy plugins.

## User interface behavior

- One pending operation disables state-changing plugin buttons and displays
  progress while the application polls a bounded result ticket.
- Initial approval and changed permissions use the same trust modal; cancel
  changes nothing.
- Enabled packages expose **Reload** and **Disable**. Faulted or stale packages
  remain stopped and offer recovery through reload or disable.
- **Open plugins folder** opens only Gradial's validated managed plugin root.
- Closing Settings dismisses an unconfirmed trust prompt. An operation already
  accepted by the worker continues safely in the background.

## Evidence

Focused tests cover code-free rescan, unloading of changed packages,
pre-evaluation approval enforcement, immediate plugin state remaining outside
the menu draft, explicit trust decisions for first enable and permission
changes, and safe error presentation. Repository-wide formatting, linting,
tests, documentation tests, and debug build are recorded in the verification
commit for this slice.

## Deferred boundary

This page manages package trust and lifecycle only. Enabled adapters remain
idle after validation because the continuous-action catalog, live
Begin/Update/Commit/Cancel routing, and capability broker are separate later
slices.
