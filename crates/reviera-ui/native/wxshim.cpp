// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// wxWidgets features that wxDragon does not wrap yet, compiled against the same wxWidgets build
// (headers and flags from its wx-config) and linked with the same static libraries.
//
// Pointers are the values wxDragon hands out (`WxWidget::handle_ptr`): the address of the object
// as created (`wxAuiNotebook*` for a notebook, `wxPanel*` for a panel).

#include <memory>
#include <wx/wx.h>
#include <wx/aui/auibook.h>
#include <wx/aui/framemanager.h>
#include <wx/aui/serializer.h>
#include <wx/aui/auibar.h>
#include <wx/bmpbndl.h>
#include <wx/display.h>
#include <wx/html/htmlwin.h>
#include <wx/ribbon/bar.h>
#include <wx/ribbon/buttonbar.h>
#include <wx/ribbon/page.h>
#include <wx/ribbon/panel.h>
#include <wx/weakref.h>

#include <map>
#include <vector>

typedef void (*reviera_html_callback)(void* user, const char* href);
typedef void (*reviera_tab_callback)(void* user, int shift);

// The help fallback where no web view is available (Wine, Windows without WebView2, ReactOS):
// wxWidgets' own HTML window. Anchors within the page scroll; every other link goes to the
// callback.
class RevieraHtml : public wxHtmlWindow {
public:
    RevieraHtml(wxWindow* parent) : wxHtmlWindow(parent, wxID_ANY) {}
    reviera_html_callback callback = nullptr;
    void* user = nullptr;
    using wxHtmlWindow::ScrollToAnchor;

    void OnLinkClicked(const wxHtmlLinkInfo& link) override {
        const wxString href = link.GetHref();
        if (href.StartsWith("#")) {
            ScrollToAnchor(href.Mid(1));
        } else if (callback) {
            callback(user, href.utf8_str().data());
        }
    }
};

extern "C" {

// A view panel takes Tab and arrow keys itself (Tab cycles what is under the cursor) instead of
// moving the focus between controls.
void reviera_wants_chars(void* window) {
    auto* w = static_cast<wxWindow*>(window);
    w->SetWindowStyleFlag((w->GetWindowStyleFlag() | wxWANTS_CHARS) & ~wxTAB_TRAVERSAL);
}

// A double click's second press reaches a view's press handlers as a press. wxWidgets sends it
// only as a double click event (wxGTK drops GDK's extra press), which would drop the second of two
// quick clicks.
void reviera_dclick_as_press(void* window) {
    auto* w = static_cast<wxWindow*>(window);
    auto pass = [w](const wxEventTypeTag<wxMouseEvent>& dclick, wxEventType down) {
        w->Bind(dclick, [w, down](wxMouseEvent& e) {
            wxMouseEvent press(e);
            press.SetEventType(down);
            press.SetEventObject(w);
            w->GetEventHandler()->ProcessEvent(press);
        });
    };
    pass(wxEVT_LEFT_DCLICK, wxEVT_LEFT_DOWN);
    pass(wxEVT_MIDDLE_DCLICK, wxEVT_MIDDLE_DOWN);
    pass(wxEVT_RIGHT_DCLICK, wxEVT_RIGHT_DOWN);
}

// Tab pressed while a view has the focus goes to `callback` (it cycles what is under the cursor)
// before wxWidgets moves the focus to the next control; elsewhere Tab navigates as usual.
void reviera_bind_tab(void* frame, reviera_tab_callback callback, void* user) {
    auto* f = static_cast<wxWindow*>(frame);
    f->Bind(wxEVT_CHAR_HOOK, [callback, user](wxKeyEvent& e) {
        if (e.GetKeyCode() == WXK_TAB && !e.ControlDown() && !e.AltDown()) {
            for (wxWindow* w = wxWindow::FindFocus(); w; w = w->GetParent()) {
                if (w->HasFlag(wxWANTS_CHARS)) {
                    callback(user, e.ShiftDown() ? 1 : 0);
                    return;
                }
            }
        }
        e.Skip();
    });
}

// Fills `panel` with a new HTML window.
void* reviera_html_create(void* panel) {
    auto* p = static_cast<wxWindow*>(panel);
    auto* h = new RevieraHtml(p);
    auto* sizer = new wxBoxSizer(wxVERTICAL);
    sizer->Add(h, 1, wxEXPAND);
    p->SetSizer(sizer, true);
    return h;
}

void reviera_html_bind(void* html, reviera_html_callback callback, void* user) {
    auto* h = static_cast<RevieraHtml*>(html);
    h->callback = callback;
    h->user = user;
}

// Shows `page` (UTF-8 HTML), at `anchor` when it is not empty.
void reviera_html_set_page(void* html, const char* page, const char* anchor) {
    auto* h = static_cast<RevieraHtml*>(html);
    h->SetPage(wxString::FromUTF8(page));
    if (anchor && *anchor) {
        h->ScrollToAnchor(wxString::FromUTF8(anchor));
    } else {
        h->Scroll(0, 0);
    }
}

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

// The view notebook's style: tabs on top, each with a close button, that can be dragged to
// reorder them, between tab groups, and to an edge to tile them (wxDragon's default style
// predates wxWidgets 3.3's flag values and leaves tab moving off).
void reviera_nb_set_style(void* notebook) {
    auto* nb = static_cast<wxAuiNotebook*>(notebook);
    nb->SetWindowStyleFlag(wxAUI_NB_TOP | wxAUI_NB_TAB_SPLIT | wxAUI_NB_TAB_MOVE |
                           wxAUI_NB_SCROLL_BUTTONS | wxAUI_NB_WINDOWLIST_BUTTON |
                           wxAUI_NB_CLOSE_ON_ALL_TABS);
    nb->Refresh();
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

// ---- Saved tile layouts: tab groups with their pages (by window) and dock sizes ----

namespace {
struct TabGroup {
    wxAuiTabLayoutInfo info;
    std::vector<wxWindow*> pages;
    std::vector<wxWindow*> pinned;
    wxWindow* active = nullptr;
};

struct BookLayout {
    std::vector<TabGroup> groups;
};

class LayoutSaver : public wxAuiBookSerializer {
public:
    LayoutSaver(wxAuiNotebook* book, BookLayout* out) : m_book(book), m_out(out) {}
    void BeforeSaveNotebook(const wxString&) override {}
    void SaveNotebookTabControl(const wxAuiTabLayoutInfo& tab) override {
        TabGroup g;
        g.info = tab;
        auto window = [&](int i) -> wxWindow* {
            return i >= 0 && size_t(i) < m_book->GetPageCount() ? m_book->GetPage(i) : nullptr;
        };
        if (tab.pages.empty()) {
            for (size_t i = 0; i < m_book->GetPageCount(); ++i)
                g.pages.push_back(m_book->GetPage(i));
        } else {
            for (int i : tab.pages)
                if (auto* w = window(i)) g.pages.push_back(w);
        }
        for (int i : tab.pinned)
            if (auto* w = window(i)) g.pinned.push_back(w);
        g.active = window(tab.active);
        m_out->groups.push_back(g);
    }

private:
    wxAuiNotebook* m_book;
    BookLayout* m_out;
};

class LayoutLoader : public wxAuiBookDeserializer {
public:
    LayoutLoader(wxAuiNotebook* book, const BookLayout* in) : m_book(book), m_in(in) {}
    std::vector<wxAuiTabLayoutInfo> LoadNotebookTabs(const wxString&) override {
        std::vector<wxAuiTabLayoutInfo> out;
        for (const auto& g : m_in->groups) {
            wxAuiTabLayoutInfo info = g.info;
            info.pages.clear();
            info.pinned.clear();
            for (auto* w : g.pages) {
                int i = m_book->GetPageIndex(w);
                if (i != wxNOT_FOUND) info.pages.push_back(i);
            }
            if (info.pages.empty()) continue;
            for (auto* w : g.pinned) {
                int i = m_book->GetPageIndex(w);
                if (i != wxNOT_FOUND) info.pinned.push_back(i);
            }
            int active = g.active ? m_book->GetPageIndex(g.active) : wxNOT_FOUND;
            info.active = active != wxNOT_FOUND ? active : info.pages.front();
            out.push_back(info);
        }
        return out;
    }

private:
    wxAuiNotebook* m_book;
    const BookLayout* m_in;
};
}

// The notebook's current layout (tab groups, their pages and sizes), and how many tab groups it
// has. Free it with reviera_nb_free_layout.
void* reviera_nb_save_layout(void* notebook, int* groups) {
    auto* book = static_cast<wxAuiNotebook*>(notebook);
    auto* layout = new BookLayout;
    LayoutSaver saver(book, layout);
    book->SaveLayout("views", saver);
    *groups = int(layout->groups.size());
    return layout;
}

// Whether every page of the notebook is in a saved layout.
int reviera_nb_layout_covers(void* notebook, const void* layout) {
    auto* book = static_cast<wxAuiNotebook*>(notebook);
    auto* l = static_cast<const BookLayout*>(layout);
    for (size_t i = 0; i < book->GetPageCount(); ++i) {
        wxWindow* w = book->GetPage(i);
        bool found = false;
        for (const auto& g : l->groups)
            for (auto* p : g.pages)
                found = found || p == w;
        if (!found) return 0;
    }
    return 1;
}

// Restores a saved layout: tab groups, page order and divider positions.
void reviera_nb_load_layout(void* notebook, const void* layout) {
    auto* book = static_cast<wxAuiNotebook*>(notebook);
    LayoutLoader loader(book, static_cast<const BookLayout*>(layout));
    book->LoadLayout("views", loader);
}

void reviera_nb_free_layout(void* layout) {
    delete static_cast<BookLayout*>(layout);
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

namespace {
// Sizes the AUI pane holding the ribbon's panel to the ribbon's height now.
void fit_ribbon_pane(wxWindow* panel, wxRibbonBar* bar) {
    wxAuiManager* m = wxAuiManager::GetManager(panel);
    if (!m) return;
    wxAuiPaneInfo& pane = m->GetPane(panel);
    if (!pane.IsOk()) return;
    bar->InvalidateBestSize();
    const int h = bar->GetBestSize().GetHeight();
    if (pane.min_size.GetHeight() == h) return;
    pane.MinSize(wxSize(-1, h)).BestSize(wxSize(-1, h)).MaxSize(wxSize(-1, h));
    m->Update();
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
    // An action may rebuild the contextual tab, destroying the button bar that sent the click
    // while wx is still handling it; so actions run once the event is done.
    // Collapsing or expanding the ribbon (its toggle button, or a double click on a tab): the
    // AUI pane holding it follows its height, else the pane keeps the full height and nothing
    // seems to happen.
    bar->Bind(wxEVT_RIBBONBAR_TOGGLED, [=](wxRibbonBarEvent& e) {
        e.Skip();
        bar->CallAfter([=] { fit_ribbon_pane(panel, bar); });
    });
    // A tab clicked while collapsed shows its page until the ribbon loses focus, and a double
    // click on the tab pins or collapses it; wx sends no event for these, so refit after them.
    auto refit = [=](wxEvent& e) {
        e.Skip();
        bar->CallAfter([=] { fit_ribbon_pane(panel, bar); });
    };
    bar->Bind(wxEVT_LEFT_DOWN, refit);
    bar->Bind(wxEVT_LEFT_DCLICK, refit);
    bar->Bind(wxEVT_KILL_FOCUS, refit);
    bar->Bind(wxEVT_RIBBONBUTTONBAR_CLICKED, [=](wxRibbonButtonBarEvent& e) {
        const int id = e.GetId();
        // A button in a collapsed panel's popup: the popup closes before the action runs (it
        // stayed open over the dialog the action opened, and after it). The popup is closed
        // through the panel it came from, which outlives it.
        wxWeakRef<wxRibbonPanel> origin;
        if (auto* rp = wxDynamicCast(e.GetBar() ? e.GetBar()->GetParent() : nullptr, wxRibbonPanel))
            origin = rp->GetExpandedDummy();
        bar->CallAfter([=] {
            if (origin) origin->HideExpanded();
            callback(user, id);
        });
    });
    return bar;
}

void* reviera_rb_page(void* bar, const char* label) {
    return new wxRibbonPage(static_cast<wxRibbonBar*>(bar), wxID_ANY, wxString::FromUTF8(label));
}

// A collapsed panel's popup at the size of its buttons. wxRibbonPanel::ShowExpanded makes a
// borderless frame at the panel's best size, but under a window manager the frame can come up
// smaller (the size of the collapsed button), and the panel in it then draws itself collapsed
// again: its label and icon, no buttons. Once shown, the frame and panel are given the best size,
// and again when the window manager resizes the frame smaller, a few times.
static void fit_expanded(wxRibbonPanel* panel) {
    wxRibbonPanel* expanded = panel->GetExpandedPanel();
    if (!expanded) return;
    wxWindow* frame = expanded->GetParent();
    expanded->InvalidateBestSize();
    const wxSize want = expanded->GetBestSize();
    if (want.x <= 0 || want.y <= 0) return;
    auto apply = [frame, expanded, want] {
        frame->SetMinClientSize(want);
        frame->SetClientSize(want);
        expanded->SetSize(0, 0, want.x, want.y);
        expanded->Layout();
        expanded->Refresh();
    };
    apply();
    auto tries = std::make_shared<int>(0);
    frame->Bind(wxEVT_SIZE, [frame, apply, want, tries](wxSizeEvent& e) {
        e.Skip();
        const wxSize now = frame->GetClientSize();
        if ((now.x < want.x || now.y < want.y) && (*tries)++ < 4) frame->CallAfter(apply);
    });
}

// A panel on `page`, returning its button bar; `svg` is the icon it shows when collapsed to its
// dropdown (empty for none).
void* reviera_rb_panel(void* page, const char* label, const char* svg) {
    auto* p = static_cast<wxRibbonPage*>(page);
    wxBitmap icon = (svg && *svg) ? svg_bundle(svg, 32).GetBitmap(wxSize(32, 32)) : wxNullBitmap;
    auto* panel = new wxRibbonPanel(p, wxID_ANY, wxString::FromUTF8(label), icon,
                                    wxDefaultPosition, wxDefaultSize,
                                    wxRIBBON_PANEL_DEFAULT_STYLE & ~wxRIBBON_PANEL_EXT_BUTTON);
    // A click on the collapsed panel opens its popup (wxRibbonPanel's own handler runs after
    // this one); its size is put right once it is shown.
    panel->Bind(wxEVT_LEFT_DOWN, [panel](wxMouseEvent& e) {
        e.Skip();
        panel->CallAfter([panel] { fit_expanded(panel); });
    });
    auto* buttons = new wxRibbonButtonBar(panel);
    g_page_bars[p].push_back(buttons);
    return buttons;
}

// A button: `large` 1 keeps icon-above-label; small buttons (0, 2) are icon beside label,
// stacked, and 2 may shrink to the icon alone (see ribbon.rs size_classes). `kind` 0 normal,
// 1 toggle.
void reviera_rb_button(void* bar, int id, const char* label, const char* svg, const char* help,
                       int large, int kind) {
    auto* b = static_cast<wxRibbonButtonBar*>(bar);
    wxBitmap big = svg_bundle(svg, 32).GetBitmap(wxSize(32, 32));
    wxBitmap small = svg_bundle(svg, 16).GetBitmap(wxSize(16, 16));
    auto k = kind == 1 ? wxRIBBON_BUTTON_TOGGLE : wxRIBBON_BUTTON_NORMAL;
    b->AddButton(id, wxString::FromUTF8(label), big, small, wxNullBitmap, wxNullBitmap, k,
                 wxString::FromUTF8(help));
    if (large == 0) {
        // Icon beside label, never icon alone: wxRibbonButtonBar's collapse of a long run to
        // icon-only columns can place them over the panel's large buttons.
        b->SetButtonMaxSizeClass(id, wxRIBBON_BUTTONBAR_BUTTON_MEDIUM);
        b->SetButtonMinSizeClass(id, wxRIBBON_BUTTONBAR_BUTTON_MEDIUM);
    } else if (large == 2) {
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

// Full-screen and focus mode (UI-DOCK-004): the menu bar and status bar hidden and shown again.
// The menu bar stays attached so its accelerators keep working: on GTK it is a widget that is
// hidden; on Windows the native menu is taken off the window; on macOS the menu bar belongs to
// the system and stays.
void reviera_frame_show_menu_bar(void* frame, int show) {
    auto* f = static_cast<wxFrame*>(frame);
    wxMenuBar* mb = f->GetMenuBar();
    if (!mb) {
        return;
    }
#if defined(__WXGTK__)
    mb->Show(show != 0);
    f->SendSizeEvent();
#elif defined(__WXMSW__)
    ::SetMenu(static_cast<HWND>(f->GetHWND()), show ? static_cast<HMENU>(mb->GetHMenu()) : nullptr);
    f->SendSizeEvent();
#else
    (void)show;
#endif
}

void reviera_frame_show_status_bar(void* frame, int show) {
    auto* f = static_cast<wxFrame*>(frame);
    if (wxStatusBar* sb = f->GetStatusBar()) {
        sb->Show(show != 0);
        f->SendSizeEvent();
    }
}

// The pointer for full-screen edge reveals: its screen position, whether a button is held, the
// edges it is pushed against (the display in full-screen, else the window) and the top and bottom
// of the views notebook on screen. out: x, y, held, left, top, right, bottom, views_top,
// views_bottom.
void reviera_pointer_state(void* frame, void* views, int* out) {
    auto* f = static_cast<wxFrame*>(frame);
    wxMouseState ms = wxGetMouseState();
    wxRect area = f->GetScreenRect();
    if (f->IsFullScreen()) {
        int d = wxDisplay::GetFromWindow(f);
        if (d != wxNOT_FOUND) {
            area = wxDisplay(static_cast<unsigned>(d)).GetGeometry();
        }
    }
    wxRect v = static_cast<wxWindow*>(views)->GetScreenRect();
    out[0] = ms.GetX();
    out[1] = ms.GetY();
    out[2] = (ms.LeftIsDown() || ms.MiddleIsDown() || ms.RightIsDown()) ? 1 : 0;
    out[3] = area.GetLeft();
    out[4] = area.GetTop();
    out[5] = area.GetRight();
    out[6] = area.GetBottom();
    out[7] = v.GetTop();
    out[8] = v.GetBottom();
}

}

