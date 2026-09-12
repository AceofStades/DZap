# Operator interface options

DZap's destructive authority stays in the root Rust backend. An operator interface should remain unprivileged and call the loopback API so device discovery, identity binding, job evidence, verification, and certificate rules have one implementation.

## Current comparison

Measurements below come from the September 2026 Arch build environment and the current release profile. They are evidence for choosing a direction, not fixed promises for future versions.

| Interface | Current state | Measured footprint | Usability | Main tradeoff |
| --- | --- | ---: | --- | --- |
| Chromium kiosk | Complete workflow | Chromium package: 416.74 MiB installed; static dashboard: 1.2 MiB | Most familiar; mouse, keyboard, long text, dialogs, downloads | Largest runtime and dependency surface. |
| Rust TUI | Device list, methods, read-only preflight | `dzap-tui`: 2.6 MiB stripped static PIE | Clear keyboard flow and works without graphics | Needs deliberate parity work for confirmation, progress, and evidence export. |
| Rust native GUI | Design option only | Not measured | Could approach the web dashboard's visual usability | Still needs graphics, input, font, and accessibility libraries; adds another UI implementation. |
| Electron | Removed | Would bundle another browser runtime | Familiar desktop widgets | Duplicates the browser/runtime model and does not serve the bootable-appliance target. |

Chromium's installed size does not equal the exact ISO savings from removing it because some graphical libraries are shared with other packages. A TUI-only image could also remove Xorg, Openbox, Mesa, VESA, and font packages after hardware testing, so the full difference must be measured by building both image profiles.

The combined kiosk-and-TUI image built on 12 September 2026 is 1,879,605,248 bytes (1,792.5 MiB). That is 10 MiB larger than the previous 10 September image, but the comparison also includes two days of rolling Arch package updates, so the increase cannot be attributed to the TUI alone. The standalone TUI binary remains the useful controlled measurement at 2.6 MiB.

## Rust TUI prototype

The ISO builds `server/src/bin/dzap-tui.rs` with the `tui` Cargo feature. It is a static x86-64 musl binary and has no additional runtime package dependency. The client runs as `dzap`, connects to `http://127.0.0.1:8080`, and asks the existing backend for:

- detected storage devices and protection state;
- methods supported for the selected device;
- the full read-only preflight decision and checks.

It does not call `POST /api/wipe`. This stop condition keeps the prototype useful for resource and navigation testing without creating a second destructive confirmation flow that lacks progress recovery and evidence export.

Controls are visible in the footer:

| Key | Action |
| --- | --- |
| Up/Down or `J`/`K` | Select a storage device. |
| Left/Right or `H`/`L` | Select one of its supported methods. |
| `Enter` or `P` | Run the backend's read-only preflight. |
| `R` | Refresh devices and capabilities. |
| `Q` or `Esc` | Exit. |

Inside the live image, tty1 remains the complete Chromium kiosk. `Ctrl+Alt+F2` opens the TUI on tty2; `Ctrl+Alt+F1` returns to the kiosk. Both clients use the same root backend while running as the unprivileged `dzap` user.

For local development, start the backend first and then run:

```bash
make run-tui
```

Set `DZAP_SERVER_ORIGIN` only when the backend uses a different loopback origin.

## Recommendation

Keep the web kiosk as the default interface for the first dependable release and retain the TUI as a recovery and low-resource candidate. The TUI is the smallest practical interface and does not depend on graphics hardware, which makes it valuable when Chromium or Xorg cannot start.

A TUI-only image becomes credible after it has identity-bound destructive confirmation, live/recovered progress, mandatory verification results, certificate generation, evidence destination selection, safe removal, and accessibility/usability testing. Implement those through the existing API rather than calling storage code directly.

A separate Rust GUI should wait until image measurements show that the web stack is a release problem and TUI usability testing shows that a console cannot meet operator needs. Building all three interfaces in parallel would multiply safety-critical wording and state-recovery work without strengthening the backend boundary.
