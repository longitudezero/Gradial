# No-side-effect meter example

This deliberately small Lua plugin requests no permissions and performs no
operating-system action. `begin` creates private session state, `update` keeps
only the latest received scalar and a count, and `commit` or `cancel` closes
that state.

To inspect and enable the package during development, copy this complete
`org.gradial.example.meter` directory into
`%LOCALAPPDATA%\Gradial\plugins`, open Gradial's **Plugins** page, choose
**Rescan plugins**, then review and enable it.

The current application does not route live radial interactions into plugin
callbacks yet, so enabling this example only validates and holds its adapter.
Its complete lifecycle is exercised by the worker test suite. The next live
bridge slice will make it usable through Gradial's built-in continuous meter
without adding an external capability.
