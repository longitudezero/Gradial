# Radial Desktop - Architecture & Design Decisions (v0.1)

## Vision

Create a desktop productivity application inspired by EVE Online's radial menu.

The application is **not** intended to replace AutoHotkey or existing automation tools. Instead, it provides a fast, visual, gesture-based interface that can trigger actions from many different backends.

The core interaction is:

> **Hold → Flick → Release**

The menu should eventually become muscle memory rather than something users consciously navigate.

---

# Design Goals

## Primary goals

* Extremely low latency
* Mouse-first workflow
* Customizable
* Extensible
* Power-user friendly
* Beautiful animations
* Context-aware


---

# Target Platform

## Version 1

* Windows 11

Reason:

Windows has stable APIs for:

* Global hotkeys
* Mouse hooks
* Keyboard hooks
* Overlay windows
* Window detection
* Input simulation
* Clipboard

Linux support can be added later once the architecture is stable.

---

# Programming Language

Rust

Reasons:

* Excellent performance
* Memory safety
* Great ecosystem
* Native binaries
* Easy future cross-platform support
* Good interoperability with native Windows APIs

---

# Overall Architecture

```
                 UI
                  │
     ┌────────────┴────────────┐
     │                              │
 Settings Window              Radial Overlay
     │                              │
     └────────────┬────────────┘
                  │
            Gesture Engine
                  │
          Profile Manager
                  │
            Action Engine
                  │
        ┌─────────┼─────────┐
        │         │         │
 Keyboard    AutoHotkey   Plugins
```

The UI should not know how actions work.

It simply tells the Action Engine:

> Execute Action X.

---

# Main Components

## 1. Overlay Renderer

Responsible for:

* Drawing radial menu
* Icons
* Animations
* Selection highlighting
* Fade in/out
* Scaling

The overlay should feel closer to a game HUD than a traditional application window.

---

## 2. Gesture Engine

Responsible for:

* Detecting hold
* Detecting mouse movement
* Calculating angle
* Selecting wedge
* Detecting release

The Gesture Engine should be completely independent from rendering.

---

## 3. Profile Manager

Chooses which radial menu to display.

Examples:

Global

VS Code

Krita

Explorer

Chrome

AutoHotkey Profile

Profiles are selected based on the active application.

---

## 4. Action Engine

Receives:

```
Copy
```

or

```
Action ID 14
```

Executes:

* Keyboard shortcut
* Macro
* Command
* AutoHotkey trigger
* Plugin action

---

# User Interface

The application has three interfaces.

## Radial Menu

The primary interface.

Displayed only while activation button is held.

---

## Settings Window

Configuration interface.

Used for:

* Profiles
* Keybindings
* Themes
* Plugins
* Updates

---

## Optional First-run Wizard

Future feature.

Not required for MVP.

---

# Radial Menu

Supports:

* 4 slices
* 8 slices
* 12 slices

Future:

* Nested radial menus

Example:

```
Files
   →
      Copy
      Rename
      Delete
```

---

# Interaction

Open

```
Hold Mouse Button
```

Navigate

```
Move Mouse
```

Execute

```
Release
```

No clicking required.

---

# Action Types

The application itself should provide built-in actions.

Examples:

* Keyboard shortcut
* Mouse click
* Clipboard
* Run executable
* Open URL
* PowerShell
* Command Prompt
* Batch file
* Delay
* Macro
* Volume
* Brightness
* Window management

Future:

* AI
* Network requests

---

# AutoHotkey Integration

AutoHotkey is considered a backend, not a competitor.

Preferred approach:

The radial menu outputs virtual keys.

Example

```
North → F13
East  → F14
South → F15
West  → F16
```

AutoHotkey decides what those keys do.

Example:

```ahk
F13::Send("^c")
F14::Send("^v")
```

Advantages:

* Existing AHK users keep their scripts.
* Minimal integration required.
* Highly flexible.

Future:

Possible IPC integration.

---

# Profiles

Profiles define:

* Layout
* Actions
* Icons
* Colors (optional)
* Activation key

Example:

```
Global

VS Code

Krita

Chrome

Explorer
```

Profiles switch automatically depending on the focused application.

---

# Skins

Skins contain no executable code.

Structure:

```
skin/

    skin.json

    background.png

    icons/

    fonts/
```

Skin controls:

* Colors
* Radius
* Icon size
* Font
* Transparency
* Borders
* Shadows
* Animations
* Background textures

Goal:

Allow complete visual customization without compromising stability.

---

# Renderer

Initially:

egui

Reason:

Rapid development.

Future:

Custom renderer if needed.

The renderer should eventually support:

* Smooth animations
* GPU acceleration
* Transparency
* Scaling
* Icon rendering

---

# Project Structure

```
src/

    main.rs

    overlay/

        renderer.rs
        radial.rs
        animation.rs

    gesture/

        detector.rs
        mouse.rs

    actions/

        keyboard.rs
        macros.rs
        ahk.rs

    plugins/

        lua.rs

    profiles/

        manager.rs

    ui/

        settings.rs

    config/

        loader.rs
```

---

# Performance Goals

Overlay opening:

<20 ms

Target FPS:

60+

Ideal:

120 FPS

Memory usage:

Minimal

The application should remain lightweight while idle.

---

# Future Features

* Linux support
* Wayland support
* Nested radial menus
* Touchscreen support
* Pen/stylus gestures
* Multiple monitors
* Radial search
* Command palette
* OBS integration
* Stream Deck integration
* Home Assistant integration
* AI actions
* Cloud profile sync
* Marketplace for skins/plugins

---

# Philosophy

The application should not try to become another AutoHotkey.

Instead, it should become the fastest and most intuitive gesture interface for desktop automation.

Automation engines (AutoHotkey, built-in actions, plugins, scripts) perform the work.

The radial menu provides a consistent, discoverable, and highly customizable interaction layer on top of them.



---

# Plugins

Plugins extend functionality.

Preferred implementation:

Lua

Reasons:

* Easy to embed
* Fast
* Cross-platform
* Small
* Large ecosystem

Future possibility:

WASM plugins.

Native DLL plugins are not recommended for initial versions due to compatibility and maintenance concerns.

---

# Plugin Philosophy

Plugins should register capabilities rather than directly modify application internals.

Example:

```
register_profile()

register_action()

register_command()

register_event()
```

---

# Plugin Events

Examples:

```
on_start()

on_shutdown()

on_profile_changed()

on_action()

on_window_changed()

on_overlay_open()

on_overlay_close()
```


---
## Plugin Architecture

The plugin system is designed around a strict separation of responsibilities.

### Core Responsibilities

The core application is responsible only for generic operating system functionality.

Examples include:

* Active window
* Process name
* Window title
* Mouse position
* Keyboard input
* Clipboard
* Selected files
* Notifications
* Overlay rendering
* Gesture recognition
* Profile management

The core should **never** contain application-specific knowledge.

For example, the core should not know concepts such as:

* Krita brush
* Photoshop layer
* VS Code workspace
* Excel worksheet
* OBS scene

These belong in application plugins.

---

## Philosophy

> **The core knows about the operating system. Plugins know about applications.**

This keeps the application:

* lightweight
* modular
* maintainable
* extensible

Users only install integrations for software they actually use.

This avoids shipping hundreds of thousands of lines of integration code for applications many users will never install.

---

# Context vs Services

The plugin API is intentionally divided into two concepts.

## Context (Read-only)

Context represents the current state of the desktop and application.

Plugins may inspect context but cannot modify it.

Example:

```rust
pub struct Context {
    process_name: String,
    window_title: String,
    mouse_position: Point,
    selected_files: Vec<PathBuf>,
    clipboard_text: Option<String>,
    current_profile: String,
}
```

Example usage:

```rust
if ctx.process_name == "Code.exe" {
    // VS Code specific behavior
}
```

The Context object should be immutable.

This provides:

* predictable behavior
* easier debugging
* safer plugins
* backwards compatibility
* easier testing

---

## Services (Capabilities)

Services allow plugins to perform actions.

Unlike Context, Services are not state.

They expose capabilities.

Example:

```rust
services.send_keys(...)

services.run_command(...)

services.show_notification(...)

services.copy_to_clipboard(...)

services.open_url(...)
```

Plugin entry point:

```rust
fn execute(context: &Context, services: &Services)
```

This cleanly separates:

* Reading information
* Performing actions

---

# Why split Context and Services?

Keeping these concepts separate makes the API significantly easier to evolve.

Context can grow with additional desktop information without affecting existing plugins.

Services can gain new capabilities without changing how plugins inspect state.

The result is a stable API that can evolve for years while remaining backwards compatible.

---

# Context Providers

Application-specific information should not live inside the core.

Instead, plugins may register Context Providers.

Example:

```text
VS Code Plugin

├── LanguageProvider
├── WorkspaceProvider
└── SelectionProvider
```

```text
Krita Plugin

├── BrushProvider
├── LayerProvider
└── CanvasProvider
```

The core application simply exposes whatever providers are currently available.

Plugins can request context by identifier rather than depending directly on another plugin.

Example:

```rust
context.get("language")

context.get("brush")

context.get("workspace")
```

If no provider exists, the request simply returns `None`.

This creates loose coupling between plugins and encourages reusable integrations.

---

# Plugin Permissions

Plugins should explicitly declare which operating system resources they require.

Example:

```toml
permissions = [
    "clipboard",
    "notifications",
    "selected_files",
    "active_window"
]
```

Users should be able to review permissions before enabling a plugin.

Future versions may support granular permission management similar to modern browser extensions.

---

# Recommended Plugin Model

The initial plugin implementation should use Lua.

Reasons:

* Small runtime
* Fast execution
* Easy embedding in Rust
* Excellent documentation
* Mature ecosystem

Future versions may optionally support WebAssembly (WASM) plugins for stronger isolation and support for multiple programming languages.

Native DLL plugins are intentionally avoided due to ABI compatibility concerns and increased maintenance complexity.

---

# Long-Term Goal

The application should become an extensible platform rather than a collection of hardcoded integrations.

The core remains intentionally small and stable.

Applications expose their own capabilities through plugins.

Plugins can build on top of other plugins using shared Context Providers instead of requiring the core to understand every desktop application.

This architecture allows the ecosystem to grow organically while keeping the base application lightweight, maintainable, and backwards compatible.

