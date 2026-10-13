// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// Native surfaces for the wgpu viewport on wxGTK. wxWindow::GetHandle() is a GtkWidget*.
//
// X11: the widget's window is made native and its XID handed to wgpu.
// Wayland: a GTK3 child widget has no wl_surface of its own, so the viewport draws into a
// separate wl_surface attached as a desynchronised subsurface of the widget's (toplevel) surface,
// kept positioned over the widget, with an empty input region so pointer events still reach GTK.
// This follows the approach of wxWidgets' own EGL canvas (src/unix/glegl.cpp).

#include <gtk/gtk.h>
#include <stdlib.h>
#include <string.h>

#ifdef GDK_WINDOWING_X11
#include <gdk/gdkx.h>
#endif
#ifdef GDK_WINDOWING_WAYLAND
#include <gdk/gdkwayland.h>
#include <wayland-client.h>
#endif

int reviera_scale_factor(void* gtk_widget) { return gtk_widget_get_scale_factor((GtkWidget*)gtk_widget); }

// 1: X11 handles returned. 0: no window yet. -1: not X11. -2: no native window.
int reviera_x11_handles(void* gtk_widget, unsigned long* xid, void** xdisplay) {
#ifdef GDK_WINDOWING_X11
    GtkWidget* widget = (GtkWidget*)gtk_widget;
    gtk_widget_realize(widget);
    GdkWindow* window = gtk_widget_get_window(widget);
    if (!window) return 0;
    if (!GDK_IS_X11_WINDOW(window)) return -1;
    if (!gdk_window_ensure_native(window)) return -2;
    *xid = gdk_x11_window_get_xid(window);
    *xdisplay = gdk_x11_display_get_xdisplay(gdk_window_get_display(window));
    return 1;
#else
    (void)gtk_widget; (void)xid; (void)xdisplay;
    return -1;
#endif
}

#ifdef GDK_WINDOWING_WAYLAND

typedef struct reviera_wl_view {
    GtkWidget* widget;
    struct wl_display* display;
    struct wl_compositor* compositor;
    struct wl_subcompositor* subcompositor;
    struct wl_surface* surface;
    struct wl_subsurface* subsurface;
    gulong map_handler, unmap_handler, size_handler, scale_handler;
} reviera_wl_view;

static void wl_global(void* data, struct wl_registry* registry, uint32_t name, const char* interface, uint32_t version) {
    reviera_wl_view* v = data;
    (void)version;
    if (!strcmp(interface, "wl_compositor"))
        v->compositor = wl_registry_bind(registry, name, &wl_compositor_interface, 3);
    else if (!strcmp(interface, "wl_subcompositor"))
        v->subcompositor = wl_registry_bind(registry, name, &wl_subcompositor_interface, 1);
}

static void wl_global_remove(void* data, struct wl_registry* registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}

static const struct wl_registry_listener registry_listener = { wl_global, wl_global_remove };

// Positions the subsurface at the widget's origin inside its toplevel.
void reviera_wayland_update(reviera_wl_view* v) {
    if (!v) return;
    wl_surface_set_buffer_scale(v->surface, gtk_widget_get_scale_factor(v->widget));
    if (!v->subsurface) return;
    GtkWidget* toplevel = gtk_widget_get_toplevel(v->widget);
    int tx = 0, ty = 0, x = 0, y = 0;
    gdk_window_get_origin(gtk_widget_get_window(toplevel), &tx, &ty);
    gdk_window_get_origin(gtk_widget_get_window(v->widget), &x, &y);
    wl_subsurface_set_position(v->subsurface, x - tx, y - ty);
}

static void create_subsurface(reviera_wl_view* v) {
    if (v->subsurface || !gtk_widget_get_mapped(v->widget)) return;
    struct wl_surface* parent = gdk_wayland_window_get_wl_surface(gtk_widget_get_window(v->widget));
    if (!parent) return;
    v->subsurface = wl_subcompositor_get_subsurface(v->subcompositor, v->surface, parent);
    wl_subsurface_set_desync(v->subsurface);
    reviera_wayland_update(v);
}

static gboolean on_map_event(GtkWidget* w, GdkEvent* e, gpointer data) {
    (void)w; (void)e;
    create_subsurface(data);
    return FALSE;
}

static void on_unmap(GtkWidget* w, gpointer data) {
    reviera_wl_view* v = data;
    (void)w;
    if (v->subsurface) {
        wl_subsurface_destroy(v->subsurface);
        v->subsurface = NULL;
    }
}

static void on_size_allocate(GtkWidget* w, GdkRectangle* a, gpointer data) {
    (void)w; (void)a;
    reviera_wayland_update(data);
}

static void on_scale(GObject* o, GParamSpec* p, gpointer data) {
    (void)o; (void)p;
    reviera_wayland_update(data);
}

// Creates the viewport surface. Returns NULL (with *code -1 when not Wayland, -2 on failure).
reviera_wl_view* reviera_wayland_create(void* gtk_widget, void** wl_display, void** wl_surface, int* code) {
    GtkWidget* widget = (GtkWidget*)gtk_widget;
    gtk_widget_realize(widget);
    GdkWindow* window = gtk_widget_get_window(widget);
    if (!window || !GDK_IS_WAYLAND_WINDOW(window)) {
        *code = -1;
        return NULL;
    }
    reviera_wl_view* v = calloc(1, sizeof *v);
    v->widget = widget;
    v->display = gdk_wayland_display_get_wl_display(gdk_window_get_display(window));
    struct wl_registry* registry = wl_display_get_registry(v->display);
    wl_registry_add_listener(registry, &registry_listener, v);
    wl_display_roundtrip(v->display);
    wl_registry_destroy(registry);
    if (!v->compositor || !v->subcompositor) {
        free(v);
        *code = -2;
        return NULL;
    }
    v->surface = wl_compositor_create_surface(v->compositor);
    struct wl_region* empty = wl_compositor_create_region(v->compositor);
    wl_surface_set_input_region(v->surface, empty);
    wl_region_destroy(empty);

    gtk_widget_add_events(widget, GDK_STRUCTURE_MASK);
    v->map_handler = g_signal_connect(widget, "map-event", G_CALLBACK(on_map_event), v);
    v->unmap_handler = g_signal_connect(widget, "unmap", G_CALLBACK(on_unmap), v);
    v->size_handler = g_signal_connect(widget, "size-allocate", G_CALLBACK(on_size_allocate), v);
    v->scale_handler = g_signal_connect(widget, "notify::scale-factor", G_CALLBACK(on_scale), v);
    create_subsurface(v);

    *wl_display = v->display;
    *wl_surface = v->surface;
    *code = 1;
    return v;
}

void reviera_wayland_destroy(reviera_wl_view* v) {
    if (!v) return;
    g_signal_handler_disconnect(v->widget, v->map_handler);
    g_signal_handler_disconnect(v->widget, v->unmap_handler);
    g_signal_handler_disconnect(v->widget, v->size_handler);
    g_signal_handler_disconnect(v->widget, v->scale_handler);
    if (v->subsurface) wl_subsurface_destroy(v->subsurface);
    wl_surface_destroy(v->surface);
    wl_subcompositor_destroy(v->subcompositor);
    wl_compositor_destroy(v->compositor);
    free(v);
}

#else

void* reviera_wayland_create(void* gtk_widget, void** wl_display, void** wl_surface, int* code) {
    (void)gtk_widget; (void)wl_display; (void)wl_surface;
    *code = -1;
    return NULL;
}
void reviera_wayland_update(void* v) { (void)v; }
void reviera_wayland_destroy(void* v) { (void)v; }

#endif
