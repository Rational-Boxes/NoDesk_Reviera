// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// Native window handles for the wgpu viewport on wxGTK. wxWindow::GetHandle() is a GtkWidget*;
// wgpu needs the X11 window and display behind it.

#include <gtk/gtk.h>
#include <gdk/gdkx.h>

// Restricts GDK to X11 (XWayland on Wayland desktops). Call before the toolkit initialises.
void reviera_prefer_x11(void) { gdk_set_allowed_backends("x11"); }

// Returns 1 with the window and display, 0 if the widget has no window yet, -1 if GDK is not using
// X11, -2 if no native window could be created.
int reviera_x11_handles(void* gtk_widget, unsigned long* xid, void** xdisplay) {
    GtkWidget* widget = (GtkWidget*)gtk_widget;
    gtk_widget_realize(widget);
    GdkWindow* window = gtk_widget_get_window(widget);
    if (!window) return 0;
    if (!GDK_IS_X11_WINDOW(window)) return -1;
    if (!gdk_window_ensure_native(window)) return -2;
    *xid = gdk_x11_window_get_xid(window);
    *xdisplay = gdk_x11_display_get_xdisplay(gdk_window_get_display(window));
    return 1;
}
