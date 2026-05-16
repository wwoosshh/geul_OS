# M4 Manual Acceptance Test

GeulOS M4 delivers a live compositor window that renders the object tree from
server-host and responds to mouse clicks by invoking object methods over the
wire.

## Prerequisites

Build all binaries in release or dev profile:

```powershell
cargo build -p geulos-server-host -p geulos-echo-app -p geulos-compositor
```

Binaries land in `target\debug\`:

| Binary              | Crate             |
|---------------------|-------------------|
| `geulosd.exe`       | geulos-server-host |
| `geulos-echo-app.exe` | geulos-echo-app  |
| `geulos-compositor.exe` | geulos-compositor |

---

## Recommended Start Order

> **Important:** The compositor performs a one-shot Query+Get on startup to
> populate its tree.  It does **not** re-query objects that mount after it
> connects.  Therefore echo-app **must** be running and its objects registered
> with the server before the compositor connects.
>
> Recommended order: **server → echo-app → compositor**

---

## 3-Terminal Procedure

### Terminal 1 — Start the server

```powershell
.\target\debug\geulosd.exe
```

Expected output:
```
[server-host] listening on 127.0.0.1:5550
```

Leave this terminal open for the duration of the test.

---

### Terminal 2 — Start the echo-app

```powershell
.\target\debug\geulos-echo-app.exe
```

Expected output:
```
[echo-app] mounted Container, Text, Button on 127.0.0.1:5550
```

(Exact wording may vary; the key signal is that the app connected and mounted
its objects without printing an error.)

Leave this terminal open.

---

### Terminal 3 — Start the compositor

```powershell
.\target\debug\geulos-compositor.exe
```

Expected output (stderr):
```
(no errors)
```

A native window titled **"GeulOS Compositor (M4)"** (800x600) should appear.

---

## What Success Looks Like

1. The compositor window renders:
   - A **Container** occupying the full window area
   - A **Text** label showing the current counter value (e.g. `"count: 0"`)
   - A **Button** widget below (or inside) the text

2. **Click the Button** with the left mouse button.

3. The Text label updates to reflect the incremented counter (e.g. `"count: 1"`).
   The update arrives via the server's `StateSet` event pushed to the compositor's
   subscription, which triggers a redraw.

4. Clicking the Button repeatedly increments the counter on each click.

5. Closing the compositor window (X button) exits cleanly without crashing any
   of the other processes.

---

## Optional 4th Terminal — geulosh invoke

`geulosh --connect` lets you send an Invoke from outside the compositor to
verify the wire path independently.

```powershell
.\target\debug\geulosh.exe --connect 127.0.0.1:5550
```

Inside the REPL:

```
geulosh> query aios.std/Button@1
# → prints the Button's object ID, e.g. "btn-<uuid>"

geulosh> invoke btn-<uuid> press
# → counter increments; compositor window redraws
```

The compositor window should show the incremented count immediately after the
invoke completes, confirming that the StateSet event propagates from the server
to all subscribers.

---

## Known Limitations

### Order sensitivity

The compositor does **not** subscribe to `Lifecycle` events that would notify
it of newly-mounted objects.  Its startup sequence is:

1. Connect to server
2. Query all known standard-type objects
3. Get each object
4. Subscribe to those objects

If echo-app starts **after** the compositor has completed step 2, none of
echo-app's objects will be in the compositor's tree and the window will appear
empty.

**Workaround:** Always start echo-app before starting the compositor, or
restart the compositor after echo-app has registered its objects.

### No dynamic object discovery

Adding or removing apps while the compositor is running will not update the
displayed tree.  A future milestone will add `Mount`/`Unmount` lifecycle event
handling.

### Single method invocation

Clicking a widget invokes only its **first** declared method.  For the Button
this is `press`, which is correct.  Widgets with multiple methods require
`geulosh` or a future context-menu UI to call non-first methods.

### Windows-only compositor

The compositor uses `winit` + `softbuffer` and has been tested on Windows 11.
Linux/macOS support is not verified for M4.
