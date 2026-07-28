# Built-in Action System (v0.1)

Instead of calling them Actions, maybe call them Nodes internally. Then a sequence becomes a graph of nodes rather than just a list of actions. That makes room later to support branching, loops, parallel execution, retries, variables, and even visual automation flows without having to rename the underlying system. The UI can still call them "Actions" for users, while the engine is built around a more flexible execution model.

## Philosophy

The core application should expose a small set of powerful, composable actions rather than hundreds of hardcoded commands.

For example, **Copy** should not be a built-in action.

Instead:

```
Copy
    ↓
Send Keyboard Shortcut
    ↓
Ctrl + C
```

This keeps the core simple while allowing users and plugins to build an unlimited number of higher-level actions.

---

# Design Principles

* Provide primitives, not workflows.
* Everything should be composable.
* Most user-facing commands are presets built on top of primitive actions.
* Plugins extend the action library rather than modifying the core.
* Actions should be reusable across profiles.

---

# Core Action Categories

## Keyboard

Responsible for generating keyboard input.

Actions:

* Send keyboard shortcut
* Press key
* Release key
* Hold key
* Type text
* Type Unicode text

Examples:

```
Ctrl + C

Ctrl + Shift + S

Alt + F4

Win + L
```

---

## Mouse

Responsible for mouse interaction.

Actions:

* Left click
* Right click
* Middle click
* Double click
* Mouse movement
* Mouse drag
* Mouse wheel scroll

Future:

* Relative movement
* Absolute movement
* Smooth movement

---

## Clipboard

Clipboard management.

Actions:

* Read clipboard
* Set clipboard
* Clear clipboard
* Append clipboard

Future:

* Clipboard history
* Clipboard manager

---

## Launch

Opening external resources.

Actions:

* Run executable
* Open file
* Open folder
* Open URL

Examples:

```
Open

https://github.com
```

```
Run

notepad.exe
```

---

## Window Management

Operating system window controls.

Actions:

* Close window
* Minimize
* Maximize
* Restore
* Toggle fullscreen
* Always on top
* Center window
* Move to monitor

Future:

* FancyZones integration
* Virtual desktops

---

## System

Operating system functionality.

Actions:

* Lock computer
* Sleep
* Restart
* Shutdown
* Log out
* Volume up
* Volume down
* Toggle mute
* Brightness up
* Brightness down

---

## Shell

Execute operating system commands.

Actions:

* Run PowerShell
* Run Command Prompt
* Run Batch file

Future:

* Windows Terminal integration

---

# Automation

Automation is built from reusable primitive actions.

Instead of creating dedicated actions such as:

```
Copy
Paste
Undo
```

Users combine primitives into sequences.

Example:

```
Delay
↓
Send Shortcut
↓
Wait
↓
Run Program
```

---

# Sequence Action

The Sequence action executes multiple actions in order.

Example:

```
Sequence

├── Send Shortcut (Ctrl+C)
├── Wait (200 ms)
├── Run Program (Notepad)
├── Paste Clipboard
└── Save File
```

Sequences become reusable actions themselves.

---

# Conditional Action

Allows branching logic without writing code.

Example:

```
If

Active Window == Chrome

↓

Open New Tab

Else

Launch Chrome
```

Future support:

* Else If
* Nested conditions
* Boolean expressions

---

# Variables

Variables allow actions to use runtime information.

Built-in variables:

* Active Window
* Process Name
* Current Profile
* Current Time
* Current Date
* Mouse Position
* Clipboard Text
* Selected Files
* Current Monitor

Example:

```
Open

C:\Logs\${CurrentDate}.txt
```

Example:

```
Type Text

Hello ${UserName}
```

---

# Plugin Actions

Plugins may register new actions.

Example:

Spotify Plugin

```
Play

Pause

Next Track

Previous Track

Mute
```

OBS Plugin

```
Start Recording

Stop Recording

Switch Scene
```

The core application does not contain any Spotify or OBS logic.

Plugins are responsible for implementing these actions.

---

# Standard Action Library

The application should ship with a library of predefined actions.

These are presets built using primitive actions.

Examples:

## Editing

* Copy
* Cut
* Paste
* Undo
* Redo
* Select All
* Save
* Save As

Internally:

```
Copy

↓

Send Shortcut

Ctrl + C
```

---

## Browser

* New Tab
* Close Tab
* Reopen Closed Tab
* Refresh
* Bookmark Page

---

## Windows

* Lock Computer
* Open Explorer
* Open Settings
* Open Task Manager
* Open Terminal

---

## Common Applications

Optional preset packs may be provided for:

* VS Code
* Chrome
* Firefox
* Microsoft Office
* Adobe applications

These remain editable by the user.

---

# Radial Menu Behavior

A radial slice should not be limited to executing a single action.

Each slice may perform one of the following:

* Execute Action
* Execute Sequence
* Open Nested Radial Menu
* Open Command Palette
* Invoke Plugin Action

This allows simple and advanced workflows to coexist.

---

# Future Features

Potential future action types:

* AI Prompt
* HTTP Request
* Webhook
* MQTT Publish
* Home Assistant
* Stream Deck Integration
* MIDI Output
* Bluetooth Device Control
* Smart Home Actions
* OCR
* Screen Capture

These should remain optional extensions rather than built-in functionality.

---

# MVP Action Set

The first version of the application should include only a small number of primitive actions.

## Required

* [ ] Send keyboard shortcut
* [ ] Type text
* [ ] Mouse action
* [ ] Run executable
* [ ] Open file
* [ ] Open folder
* [ ] Open URL
* [ ] Window management
* [ ] System controls
* [ ] Delay
* [ ] Sequence
* [ ] Conditional action
* [ ] Variable expansion
* [ ] Plugin action

Everything else should be built from these primitives.

This keeps the core application small, predictable, and easy to extend while enabling users and plugins to create sophisticated workflows.
