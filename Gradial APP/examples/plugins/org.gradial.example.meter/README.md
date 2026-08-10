# No-side-effect meter example

This deliberately small Lua plugin requests no permissions and performs no
operating-system action. `begin` creates private session state, `update` keeps
only the latest received scalar and a count, and `commit` or `cancel` closes
that state.

To inspect and enable the package during development, copy this complete
`org.gradial.example.meter` directory into
`%LOCALAPPDATA%\Gradial\plugins`, open Gradial's **Plugins** page, choose
**Rescan plugins**, then review and enable it.

After enabling it, create its continuous action in Settings and assign that
action to a continuous-enabled wedge. It then receives the live radial value
without requesting an external capability. Its complete lifecycle is also
exercised by the worker test suite.
