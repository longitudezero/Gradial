# Loopback commit example

This plugin demonstrates one bounded `gradial.network.loopback(...)` request.
It sends a JSON value only when its continuous gesture commits. Cancel changes
private session state only and cannot enqueue network work.

The manifest requests review for exactly `http://127.0.0.1:32123` with `POST`.
Changing the address, port, or method disables the plugin until those changes
are reviewed again. Gradial never resolves a hostname, follows a redirect,
uses a proxy, or gives the response body to Lua.

The example expects a local HTTP service to accept `POST /gradial/example`.
Without that service, the request fails safely within 250 ms and the continuous
action reports a capability failure. No service is bundled or started by this
example.

For manual author testing, copy the complete
`org.gradial.example.loopback` directory into Gradial's local `plugins`
directory, rescan, review the exact destination, enable the plugin, create its
continuous action, and assign it to a wedge with continuous mode enabled.
