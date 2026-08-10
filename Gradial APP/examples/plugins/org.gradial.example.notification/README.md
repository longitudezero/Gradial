# Commit notification example

This deliberately small Lua plugin requests one plain Gradial notification
when its continuous gesture commits. It demonstrates the protected
`gradial.notification(text)` API without including a link, markup, action
button, or application-specific behavior.

Gradial identifies the plugin in the native notification. The request is
limited to one per gesture and five per plugin per rolling minute. A Cancel
callback cannot submit a notification, and this example's Cancel callback only
updates private Lua state.

To try it during development, copy this complete
`org.gradial.example.notification` directory into
`%LOCALAPPDATA%\Gradial\plugins`, open Gradial's **Plugins** page, choose
**Rescan plugins**, review and enable it, then create its continuous action and
assign that action to a continuous-enabled wedge.
