# Specification for circular center-click menu

Center click radial menu. ON center click center the mouse pointer
in the view. The radial menu is a ring around the mouse pointer
with a maximum of 8 items. A left click dismisses the menu. When the
mouse pointer touches a menu item, it re-centers and the ring updates
to the next tier of menus.

Center click on an element presents the same menus as currently. Center
click on the background opens a menu set that mirrors the main menu
and ribbon items.

Make the radial menu a user controllable application feature, it is
experimental.

## Full-screen mode

Option to go full-screen on F11. When in full-screen pushing the mouse
pointer against the top will show the menu and ribbon menus, and the
exit full-screen button.

Pushing against the bottom of the screen cause the palette menus to appear
and clicking on the model dismisses the panels. 

## Decisions (2026-10-20)

Written into the specification as UI-SEL-007 *Radial menu (experimental)* (Chapter 1 §1.21.7)
and UI-DOCK-004 *Hiding chrome* (Chapter 1 §1.15.2). Owner answers to the follow-up questions:

- The ring moves to the pointer; the pointer is never warped.
- Touching a leaf item highlights it; a left click runs it.
- F11 is full-screen; focus mode (Ctrl+Shift+F11) merges into it as the same state without OS
  full-screen. Background tasks moves from F11 to Ctrl+Alt+T.
- The bottom edge reveals all panes, with the status bar, as an overlay over the canvas.

Recommended defaults, confirmed by the owner: radial menu off by default (Options › User
Interface › Experimental); a More… wedge past 7 items; moving back to the centre returns a tier;
Esc, a right click or a left click elsewhere dismisses; arrow keys, Enter and Backspace in the
ring; background tiers are ribbon tabs › panels › tools plus a Menu wedge; reveal dwell 150 ms
(500 ms on an edge shared with another monitor); bottom overlay up to 40 % of the canvas height;
no reveal while a mouse button is held; Ctrl+Cmd+F for full-screen on macOS.
