# Milestone 2 action feedback results

Date: 2026-07-29

This record covers the bounded Milestone 2 action-feedback slice: recent
failure/cancellation activity in the notification-area menu and
non-focus-stealing Windows notifications for failures. Every schema-two action
backend remains enabled.

## Feedback boundary

Normal worker completions and immediate coordinator rejections now enter the
same feedback path. `Success` produces no activity and no notification.
`Cancelled` produces one recent-activity entry but no notification. `Failure`
produces one activity entry and queues one Windows notification.

The feedback layer accepts an `ActionResult`, but it does not retain or display
the result's `safe_message`. Instead, each stable `ActionErrorCode` maps to a
fixed, non-technical public message inside the tray module. This creates a
defensive privacy boundary: action text, URLs, paths, executable arguments,
working directories, and target titles cannot enter the activity panel or
notification through a dynamic error string.

## Bounded session activity

Activity exists only in process memory. It retains the latest 20 failures and
cancellations, newest first, and evicts the oldest entry when full. The
notification-area menu exposes a **Recent activity (N)** submenu, an explicit
empty state, and **Clear activity**. Clearing removes both the visible history
and any failure notification not yet submitted to Windows. Restarting Gradial
also clears the history; no activity file or registry value exists.

The input-health status, **Retry input**, **Cancel current action**, and **Exit
Gradial** behavior remain unchanged.

## Windows notification behavior

Failures are submitted through the identity of Gradial's existing
notification-area icon with `Shell_NotifyIconW(NIF_INFO)`. The request contains
only a fixed title and the error-code message. It requests the Windows error
glyph, suppresses notification sound, and respects quiet time. The notification
call does not show, activate, or focus Gradial's controller window.

Submission is best effort. If Explorer cannot accept the notification, the
failure remains in recent activity and Gradial records the notification error
locally without retrying or blocking action execution.

## Automated coverage

Unit tests prove:

- success creates neither activity nor a queued notification;
- cancellation creates activity but no notification;
- the store retains exactly the latest 20 records;
- clearing removes retained activity and pending notifications;
- the native **Recent activity (2)** submenu contains both records and an
  enabled **Clear activity** command;
- a deliberately private `safe_message` cannot reach the menu or notification
  text;
- notification data uses only the non-activating information flag together
  with the error, no-sound, and quiet-time options;
- title and message buffers contain only the fixed public strings.

The existing executor and interaction tests continue to cover normal
completion, cancellation, timeouts, busy rejection, and shutdown. Immediate
dispatch rejection is explicitly connected to the same feedback path rather
than being lost outside the worker completion queue.

## Isolated Windows evidence

`smoke-action-feedback.ps1` creates a temporary release configuration whose
eight slices all reference one missing `.exe`. The missing path contains a
private sentinel and the action contains a private argument. One synthetic M4
gesture must:

- compile and dispatch the action;
- finish with `path_not_found`;
- add exactly one retained failure record;
- submit exactly one Windows failure notification successfully;
- preserve the foreground application through the post-notification check;
- omit both private values from the isolated Gradial log.

The fixture never changes the user's Gradial configuration and removes its
temporary local-data directory afterward.

The isolated failure run completed its normal close lifecycle in 50 ms. A
separate isolated Delay-success run completed in 51 ms and confirmed that a
successful plan produced neither an activity record nor a failure
notification.

## Validation evidence

The completed implementation passed:

- 134 debug workspace tests and 132 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- formatting and PowerShell syntax validation for every development script;
- the isolated missing-executable activity, notification, foreground-focus,
  and log-privacy check;
- an isolated successful Delay regression proving that success remains silent.

## Current boundary

There is no settings toggle for failure notifications yet because the settings
editor has not been implemented. Activity is intentionally session-only and
does not include timestamps, action names, payload previews, or success
history. Notification clicks have no destination until a non-technical
settings/activity window exists.
