# Milestone 2 profile matching editor results

Date: 2026-07-30

This record covers the advanced application-profile matching slice: optional
window-class and title predicates, inline rule validation, a countdown-based
foreground capture, complete production-resolver explanation, privacy cleanup,
persistence, and live reload.

## Advanced rule controls

The existing exact executable path remains required for every application
profile. The Profiles tab now also exposes two opt-in predicates:

- **Match window class** performs a case-insensitive exact comparison.
- **Match window title pattern** compiles a case-sensitive Rust regular
  expression.

Each checkbox directly controls the corresponding optional configuration
field. Turning a match off removes that predicate; turning it on exposes its
text field. Empty enabled fields show an inline message and disable save.
Invalid title patterns are rejected inline and again by authoritative complete
configuration validation before disk mutation.

Profile duplication continues to copy the complete rule. Rename, executable,
priority, and menu edits leave enabled class/title predicates unchanged.
Runtime semantics remain in `ProfileResolver`; the editor does not implement a
second matching algorithm.

## Private target capture

**Capture target in 3 seconds** starts a visible countdown and repeatedly
requests only settings-window repaint. The user can switch to the application
they want to test. At the deadline, Gradial reads:

- the foreground process's complete executable path when Windows permits it;
- the foreground window class;
- the foreground window title.

The capture does not focus or reactivate Gradial. It can be cancelled before
the deadline or replaced by another capture.

Captured values remain only in `SettingsState`. They are not inserted into the
configuration draft, save candidate, recent activity, or diagnostics. The
tester has an explicit **Clear captured details** command. A clean close and a
confirmed discard close also clear the retained target; reopening settings
always starts without a capture. Its debug formatter reports only whether each
field is available, preventing accidental diagnostic formatting from exposing
the values.

## Resolver explanation

After capture, the current complete draft is compiled through the production
`ProfileResolver`. The tester displays:

- the captured path, class, and title;
- every application profile that matched all enabled predicates;
- each match's configured priority, predicate specificity, and immutable UUID;
- the winning profile or Global;
- the exact decision step.

The decision text distinguishes unavailable executable identity, no matching
application profile, sole match, highest priority, highest specificity, and
the lexicographically smallest UUID tie-break. Because the target is retained
but the resolver is rebuilt from the current draft on every frame, editing a
predicate or priority updates the explanation immediately.

If any part of the complete draft is invalid, the tester retains the private
capture but asks the user to fix the highlighted settings before resolving.
It never offers an approximate result from a partially valid rule set.

## Validation and persistence

Draft validation now maps empty window classes, empty title patterns, and
invalid title regular expressions to separate profile-specific messages.
Complete configuration validation and resolver compilation repeat before the
existing atomic replacement. A successful save preserves the optional
predicates, advances `config_revision`, and installs the rebuilt resolver so
fresh M4 invocations use the edited rule immediately.

An invocation already open remains tied to its frozen profile and menu.
Testing a target never changes the running resolver or invocation state.

## Automated and native evidence

Focused settings tests verify:

- empty enabled class matching maps to its own profile field;
- empty and invalid title patterns remain distinct;
- the three-second display rounds partial seconds upward;
- every resolver decision has an explicit user-facing explanation;
- closing a clean editor purges a retained private target.

Existing core tests continue to prove path and class case normalization,
case-sensitive title matching, priority ordering, specificity ordering, UUID
tie-breaking, and Global fallback.

The isolated native settings harness:

1. creates a Notepad profile with class `Notepad`, a title pattern, priority
   72, and its independent eight-slice menu;
2. runs the real three-second capture timer and foreground metadata path;
3. displays a deterministic private matching target through the real settings
   surface and production resolver;
4. persists the exact optional predicates through revision 2;
5. asserts that the captured path, class, and private title do not appear in
   Gradial's local log;
6. retains the profile-free prior revision in `config.json.bak`;
7. executes a fresh action through the reloaded configuration.

The fixture remains isolated from the user's configuration.

Final verification passed:

- 167 debug workspace tests and 165 release workspace tests;
- Clippy with warnings denied for every target and feature in debug and release;
- debug and release builds;
- formatting and PowerShell syntax checks;
- the expanded native advanced-rule, countdown, resolver, privacy, save,
  backup, and live-reload workflow;
- one live release M4 Delay invocation.

## Current boundary

Application-profile lifecycle, complete menus, exact executable matching,
optional class/title predicates, deterministic priority, and current-target
explanation are now editable end to end. Activation binding, appearance,
icons, import/export, and onboarding remain future bounded slices.
