# Banded shortcut example

This small Lua plugin demonstrates Gradial's `input.shortcut` capability. It
divides the continuous value into a configurable number of bands and asks
Gradial to send one complete shortcut when the value moves toward another
band. It submits at most one request per callback, so a large jump catches up
over later updates instead of producing an input burst.

The manifest deliberately offers a short list of harmless, readable key names.
The default Right/Left pair can move a media player's timeline when those keys
are its seek controls. Different applications may interpret the same keys in
different ways.

The plugin must be reviewed because it requests `input.shortcut`. Gradial
checks that permission again on every request, targets only the window frozen
at invocation-down, and sends a complete press/release chord. The `cancel`
callback only updates private Lua state and cannot submit input.

To try it during development, copy this complete
`org.gradial.example.shortcut-bands` directory into
`%LOCALAPPDATA%\Gradial\plugins`, open Gradial's **Plugins** page, choose
**Rescan plugins**, review and enable it, then create its continuous action and
assign that action to a continuous-enabled wedge.
