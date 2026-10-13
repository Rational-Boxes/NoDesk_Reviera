// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// wxWidgets features that wxDragon does not wrap yet, compiled against the same wxWidgets build
// (headers and flags from its wx-config) and linked with the same static libraries.
//
// Pointers are the values wxDragon hands out (`WxWidget::handle_ptr`): the address of the object
// as created (`wxAuiNotebook*` for a notebook, `wxPanel*` for a panel).

#include <wx/wx.h>
#include <wx/aui/auibook.h>

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

}
