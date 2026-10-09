# Phase 1 design review

Interactive preview: http://127.0.0.1:1420. The images use sample data and a fixed October 9, 2026 review date.

| View                  | Light                            | Dark                           |
| --------------------- | -------------------------------- | ------------------------------ |
| Month                 | [Light month](month-light.png)   | [Dark month](month-dark.png)   |
| Week and overlaps     | [Light week](week-light.png)     | [Dark week](week-dark.png)     |
| Day                   | [Light day](day-light.png)       | [Dark day](day-dark.png)       |
| Anchored event editor | [Light editor](editor-light.png) | [Dark editor](editor-dark.png) |

Additional scenes: [empty](month-empty.png), [dense](month-dense.png), [overflow list](month-overflow.png), [900×600 window](month-compact.png), and [narrow tiled window](window-tiled.png).

The light palette's smallest secondary-text token has a 4.76:1 contrast ratio against the sidebar background. Calendar-event text exceeds 6:1 in light and 7:1 in dark. Keyboard focus remains visible; motion is disabled when requested by the OS.

This is the planned design acceptance gate. Give feedback on spacing, typography, color, calendar density, and the editing flow before phase 2 introduces the permanent local calendar.
