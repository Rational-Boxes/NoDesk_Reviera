// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// wxWidgets features that wxDragon does not wrap yet, compiled against the same wxWidgets build
// (headers and flags from its wx-config) and linked with the same static libraries.
//
// Pointers are the values wxDragon hands out (`WxWidget::handle_ptr`): the address of the object
// as created (`wxAuiNotebook*` for a notebook, `wxPanel*` for a panel).

#include <wx/wx.h>
#include <wx/aui/auibook.h>
#include <wx/aui/auibar.h>
#include <wx/bmpbndl.h>
#include <wx/ribbon/bar.h>
#include <wx/ribbon/buttonbar.h>
#include <wx/ribbon/page.h>
#include <wx/ribbon/panel.h>

#include <map>
#include <vector>

extern "C" {

typedef void (*reviera_nb_callback)(void* user, int kind, int page);

enum {
    REVIERA_NB_CHANGED = 1,
    REVIERA_NB_CLOSE = 2,
    REVIERA_NB_MIDDLE_UP = 3,
    REVIERA_NB_RIGHT_UP = 4,
};

// Forwards notebook events. Closing is always vetoed: the application decides, cleans up the
// page's rendering surface, and deletes the page itself.
void reviera_nb_bind(void* notebook, reviera_nb_callback callback, void* user) {
    auto* nb = static_cast<wxAuiNotebook*>(notebook);
    nb->Bind(wxEVT_AUINOTEBOOK_PAGE_CHANGED, [=](wxAuiNotebookEvent& e) {
        e.Skip();
        callback(user, REVIERA_NB_CHANGED, e.GetSelection());
    });
    nb->Bind(wxEVT_AUINOTEBOOK_PAGE_CLOSE, [=](wxAuiNotebookEvent& e) {
        e.Veto();
        callback(user, REVIERA_NB_CLOSE, e.GetSelection());
    });
    nb->Bind(wxEVT_AUINOTEBOOK_TAB_MIDDLE_UP, [=](wxAuiNotebookEvent& e) {
        callback(user, REVIERA_NB_MIDDLE_UP, e.GetSelection());
    });
    nb->Bind(wxEVT_AUINOTEBOOK_TAB_RIGHT_UP, [=](wxAuiNotebookEvent& e) {
        callback(user, REVIERA_NB_RIGHT_UP, e.GetSelection());
    });
}

// Moves `page` into a new tab group at `direction` (1 left, 2 right, 3 top, 4 bottom).
void reviera_nb_split(void* notebook, size_t page, int direction) {
    auto* nb = static_cast<wxAuiNotebook*>(notebook);
    static const int dirs[] = {wxLEFT, wxLEFT, wxRIGHT, wxTOP, wxBOTTOM};
    if (page < nb->GetPageCount() && direction >= 1 && direction <= 4) nb->Split(page, dirs[direction]);
}

// Merges every tab group back into one.
void reviera_nb_unsplit_all(void* notebook) {
    static_cast<wxAuiNotebook*>(notebook)->UnsplitAll();
}

// The index of `page`, or -1.
int reviera_nb_page_index(void* notebook, void* page) {
    return static_cast<wxAuiNotebook*>(notebook)
        ->GetPageIndex(static_cast<wxWindow*>(page));
}


// ---- Ribbon (UI-RIB-001): wxRibbonBar pages, panels and button bars ----

typedef void (*reviera_rb_callback)(void* user, int id);

namespace {
// The button bars holding each tool id, to enable or toggle every copy of a tool.
std::multimap<int, wxRibbonButtonBar*> g_buttons;
// The button bars of each page, to forget them when the page is cleared.
std::map<wxRibbonPage*, std::vector<wxRibbonButtonBar*>> g_page_bars;

wxBitmapBundle svg_bundle(const char* svg, int size) {
    return wxBitmapBundle::FromSVG(svg, wxSize(size, size));
}
}

// A ribbon filling `parent` (a panel docked by AUI), forwarding button clicks to `callback`.
void* reviera_rb_new(void* parent, reviera_rb_callback callback, void* user) {
    auto* panel = static_cast<wxWindow*>(parent);
    auto* bar = new wxRibbonBar(panel, wxID_ANY, wxDefaultPosition, wxDefaultSize,
                                wxRIBBON_BAR_FLOW_HORIZONTAL | wxRIBBON_BAR_SHOW_PAGE_LABELS |
                                    wxRIBBON_BAR_SHOW_PANEL_EXT_BUTTONS |
                                    wxRIBBON_BAR_SHOW_TOGGLE_BUTTON);
    auto* sizer = new wxBoxSizer(wxVERTICAL);
    sizer->Add(bar, 1, wxEXPAND);
    panel->SetSizer(sizer);
    bar->Bind(wxEVT_RIBBONBUTTONBAR_CLICKED,
              [=](wxRibbonButtonBarEvent& e) { callback(user, e.GetId()); });
    return bar;
}

void* reviera_rb_page(void* bar, const char* label) {
    return new wxRibbonPage(static_cast<wxRibbonBar*>(bar), wxID_ANY, wxString::FromUTF8(label));
}

// A panel on `page`, returning its button bar.
void* reviera_rb_panel(void* page, const char* label) {
    auto* p = static_cast<wxRibbonPage*>(page);
    auto* panel = new wxRibbonPanel(p, wxID_ANY, wxString::FromUTF8(label), wxNullBitmap,
                                    wxDefaultPosition, wxDefaultSize,
                                    wxRIBBON_PANEL_DEFAULT_STYLE & ~wxRIBBON_PANEL_EXT_BUTTON);
    auto* buttons = new wxRibbonButtonBar(panel);
    g_page_bars[p].push_back(buttons);
    return buttons;
}

// A button: `large` keeps icon-above-label when there is room; small buttons are icon beside
// label, stacked. `kind` 0 normal, 1 toggle.
void reviera_rb_button(void* bar, int id, const char* label, const char* svg, const char* help,
                       int large, int kind) {
    auto* b = static_cast<wxRibbonButtonBar*>(bar);
    wxBitmap big = svg_bundle(svg, 32).GetBitmap(wxSize(32, 32));
    wxBitmap small = svg_bundle(svg, 16).GetBitmap(wxSize(16, 16));
    auto k = kind == 1 ? wxRIBBON_BUTTON_TOGGLE : wxRIBBON_BUTTON_NORMAL;
    b->AddButton(id, wxString::FromUTF8(label), big, small, wxNullBitmap, wxNullBitmap, k,
                 wxString::FromUTF8(help));
    if (!large) {
        b->SetButtonMaxSizeClass(id, wxRIBBON_BUTTONBAR_BUTTON_MEDIUM);
    } else {
        b->SetButtonMinSizeClass(id, wxRIBBON_BUTTONBAR_BUTTON_LARGE);
    }
    g_buttons.emplace(id, b);
}

// Removes every panel of `page` (to fill it again).
void reviera_rb_clear_page(void* page) {
    auto* p = static_cast<wxRibbonPage*>(page);
    for (auto* b : g_page_bars[p]) {
        for (auto it = g_buttons.begin(); it != g_buttons.end();) {
            it = it->second == b ? g_buttons.erase(it) : std::next(it);
        }
    }
    g_page_bars.erase(p);
    p->DestroyChildren();
}

void reviera_rb_set_page_label(void* page, const char* label) {
    static_cast<wxRibbonPage*>(page)->SetLabel(wxString::FromUTF8(label));
}

void reviera_rb_realize(void* bar) {
    static_cast<wxRibbonBar*>(bar)->Realize();
}

void reviera_rb_enable(int id, int enable) {
    auto range = g_buttons.equal_range(id);
    for (auto it = range.first; it != range.second; ++it) it->second->EnableButton(id, enable != 0);
}

void reviera_rb_toggle(int id, int on) {
    auto range = g_buttons.equal_range(id);
    for (auto it = range.first; it != range.second; ++it) it->second->ToggleButton(id, on != 0);
}

// Shows or hides page `index`; shown contextual pages are highlighted.
void reviera_rb_show_page(void* bar, int index, int show, int highlight) {
    auto* b = static_cast<wxRibbonBar*>(bar);
    b->ShowPage(index, show != 0);
    if (show && highlight) {
        b->AddPageHighlight(index);
    } else {
        b->RemovePageHighlight(index);
    }
}

void reviera_rb_set_active(void* bar, int index) {
    static_cast<wxRibbonBar*>(bar)->SetActivePage(static_cast<size_t>(index));
}

int reviera_rb_active(void* bar) {
    return static_cast<wxRibbonBar*>(bar)->GetActivePage();
}

int reviera_rb_best_height(void* bar) {
    return static_cast<wxRibbonBar*>(bar)->GetBestSize().GetHeight();
}

// Sets an AUI toolbar tool's icon from SVG.
void reviera_aui_tool_svg(void* toolbar, int id, const char* svg, int size) {
    auto* tb = static_cast<wxAuiToolBar*>(toolbar);
    tb->SetToolBitmap(id, svg_bundle(svg, size));
}

}
