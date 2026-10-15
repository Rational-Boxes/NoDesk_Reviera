// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Help (UI-HELP-001): hypertext help pages in a tab of the view notebook, beside the model's
//! views. Help › Reviera Help opens the landing page (or shows it when it is open). Its links:
//! - `reviera:help/<page>` (optionally `#anchor`) shows another help page in the same tab, and
//!   `reviera:help-back` goes back to the page before;
//! - `reviera:sample/<name>` opens a fresh copy of a sample model (`test-data/samples`), so the
//!   sample stays as shipped;
//! - `reviera:action/<id>` runs an action, as the ribbon would;
//! - `#anchor` moves within the page; web links open in the system browser.
//!
//! A small script in the page catches those clicks and hands the link over through the page's
//! title, which works with every webview backend.
//!
//! Where there is no web view (Windows without the WebView2 runtime: Wine, older Windows,
//! ReactOS), the pages are shown in wxWidgets' own HTML window instead, without the stylesheet
//! and script; its links work the same. `REVIERA_HELP_FALLBACK=1` forces it, for testing.
//!
//! The pages are HTML compiled into the application: [`PAGES`] lists them, each a body shown in
//! one frame (`help/shell.html`) with its header, stylesheet and link script. Adding a page is a
//! line in [`PAGES`] and its `help/<slug>.html`. The landing page's samples section is built
//! from the samples folder's `samples.json` and each script's `-- description:` line, and its
//! contents list from [`PAGES`].

use std::path::{Path, PathBuf};

use wxdragon::event::WebViewEvents;
use wxdragon::prelude::*;
use wxdragon::widgets::{WebView, WebViewBackend};

use super::{Gui, error, with};

/// The frame of every page, with `{{TITLE}}`, `{{BACK}}`, `{{ANCHOR}}` and `{{BODY}}` to fill in.
const SHELL: &str = include_str!("../help/shell.html");

/// A help page: its slug (`reviera:help/<slug>`), title, the contents group it is listed in
/// (`None` for the landing page) and its body.
pub(crate) struct Page {
    pub slug: &'static str,
    pub title: &'static str,
    pub group: Option<&'static str>,
    pub body: &'static str,
}

const CORE: Option<&str> = Some("Using Reviera");
const ARCHITECTURE: Option<&str> = Some("Architecture");

macro_rules! page {
    ($slug:literal, $title:literal, $group:expr) => {
        Page {
            slug: $slug,
            title: $title,
            group: $group,
            body: include_str!(concat!("../help/", $slug, ".html")),
        }
    };
}

/// The help pages, in contents order. The landing page (`index`, with `{{VERSION}}`,
/// `{{SAMPLES}}` and `{{CONTENTS}}` to fill in) comes first.
pub(crate) const PAGES: &[Page] = &[
    page!("index", "Reviera Help", None),
    page!("start", "Getting started", CORE),
    page!("window", "The window", CORE),
    page!("views", "Views and navigation", CORE),
    page!("selecting", "Selecting and modifying", CORE),
    page!("settings", "Model properties and units", CORE),
    page!("types", "Types and parameters", CORE),
    page!("coordination", "Overlaps, booleans and clearances", CORE),
    page!("documentation", "Documentation", CORE),
    page!("scripting", "Scripting and automation", CORE),
    page!("levels-grids", "Levels and grids", ARCHITECTURE),
    page!("walls", "Walls", ARCHITECTURE),
    page!("curtain-walls", "Curtain walls", ARCHITECTURE),
    page!("doors-windows", "Doors and windows", ARCHITECTURE),
    page!("columns", "Columns", ARCHITECTURE),
    page!("components", "Components and libraries", ARCHITECTURE),
    page!("sketch", "Sketch mode", ARCHITECTURE),
    page!("floors-ceilings", "Floors and ceilings", ARCHITECTURE),
    page!("roofs", "Roofs", ARCHITECTURE),
    page!("openings", "Openings", ARCHITECTURE),
    page!("skylights", "Skylights", ARCHITECTURE),
    page!("stairs", "Stairs", ARCHITECTURE),
    page!("ramps", "Ramps", ARCHITECTURE),
    page!("railings-decks", "Railings and decks", ARCHITECTURE),
    page!("rooms", "Rooms", ARCHITECTURE),
    page!("massing", "Massing and model by face", ARCHITECTURE),
    page!("site", "Site: toposolids and property lines", ARCHITECTURE),
    page!("parts", "Parts", ARCHITECTURE),
];

/// The page with `slug`.
pub(crate) fn page(slug: &str) -> Option<&'static Page> {
    PAGES.iter().find(|p| p.slug == slug)
}

/// A place in the help: a page and an anchor in it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Place {
    pub slug: String,
    pub anchor: Option<String>,
}

impl Place {
    fn home() -> Place {
        Place {
            slug: "index".to_owned(),
            anchor: None,
        }
    }
}

/// How many pages Back remembers.
const BACK_LIMIT: usize = 50;

/// The open help tab.
pub struct HelpTab {
    pub panel: Panel,
    view: HelpView,
    /// The page shown.
    here: Place,
    /// The pages shown before, the latest last (`reviera:help-back`).
    back: Vec<Place>,
}

/// What shows the pages.
#[derive(Clone, Copy)]
enum HelpView {
    Web(WebView),
    /// No web view: wxWidgets' HTML window.
    Html(super::wx::HtmlView),
}

/// Whether help needs the HTML window: Windows without WebView2 (the IE backend that is left
/// does not run the pages' script).
fn needs_fallback() -> bool {
    if std::env::var_os("REVIERA_HELP_FALLBACK").is_some_and(|v| v == "1") {
        return true;
    }
    cfg!(windows) && !WebView::is_backend_available(WebViewBackend::Edge)
}

/// The help colours (the stylesheet's, light and dark), for the HTML window.
struct Palette {
    ink: &'static str,
    muted: &'static str,
    line: &'static str,
    accent: &'static str,
    paper: &'static str,
    card: &'static str,
    code: &'static str,
}

const LIGHT: Palette = Palette {
    ink: "#23272e",
    muted: "#5b6370",
    line: "#d8dde4",
    accent: "#2a6edc",
    paper: "#f7f8fa",
    card: "#ffffff",
    code: "#eef1f5",
};

const DARK: Palette = Palette {
    ink: "#e6e8eb",
    muted: "#a3abb8",
    line: "#3a404a",
    accent: "#6ea2ff",
    paper: "#1f2329",
    card: "#282d34",
    code: "#2f353d",
};

/// A page for the HTML window, which knows HTML 3.2 but no CSS: the stylesheet's look redone
/// with attributes (colours, the header bar, rules under headings, muted first columns, cards
/// for the samples, shaded code blocks), and without the script.
fn plain_html(html: &str, dark: bool) -> String {
    let c = if dark { &DARK } else { &LIGHT };
    let between = |a: &str, b: &str| -> String {
        html.split_once(a)
            .and_then(|(_, rest)| rest.split_once(b))
            .map(|(x, _)| x.to_owned())
            .unwrap_or_default()
    };
    let title = between("<title>", "</title>");
    let back = if html.contains("reviera:help-back") {
        "&nbsp;&nbsp;&nbsp;<a href=\"reviera:help-back\">Back</a>"
    } else {
        ""
    };
    let mut body = between("<main>", "</main>");
    let muted =
        |inner: &str, size: &str| format!("<font color=\"{}\"{size}>{inner}</font>", c.muted);
    // Paragraph classes.
    for (class, size) in [
        ("lead", ""),
        ("note", " size=\"-1\""),
        ("see", " size=\"-1\""),
    ] {
        let open = format!("<p class=\"{class}\">");
        let mut out = String::new();
        let mut rest = body.as_str();
        while let Some(i) = rest.find(&open) {
            out.push_str(&rest[..i]);
            let after = &rest[i + open.len()..];
            let j = after.find("</p>").unwrap_or(after.len());
            out.push_str(&format!("<p>{}</p>", muted(&after[..j], size)));
            rest = after.get(j + 4..).unwrap_or("");
        }
        out.push_str(rest);
        body = out;
    }
    // A rule under each section heading.
    body = body.replace(
        "</h2>",
        &format!("</h2><hr size=\"1\" color=\"{}\">", c.line),
    );
    // Tables: full width, padded; the first cell of a row muted.
    body = body.replace("<table class=\"keys\">", "<table>");
    body = body.replace(
        "<table>",
        "<table width=\"100%\" cellpadding=\"5\" cellspacing=\"0\" border=\"0\">",
    );
    let mut out = String::new();
    let mut rest = body.as_str();
    while let Some(i) = rest.find("<tr>") {
        out.push_str(&rest[..i + 4]);
        let after = &rest[i + 4..];
        let lead = after.len() - after.trim_start().len();
        let row = &after[lead..];
        if let Some(cell) = row.strip_prefix("<td>")
            && let Some(j) = cell.find("</td>")
        {
            out.push_str(&format!(
                "<td width=\"32%\" valign=\"top\">{}</td>",
                muted(&cell[..j], "")
            ));
            rest = &cell[j + 5..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    body = out.replace("<td>", "<td valign=\"top\">");
    // Sample cards.
    body = body
        .replace(
            "<div class=\"sample\">",
            &format!(
                // A one-pixel frame in the line colour: a table inside a table.
                "<table width=\"100%\" cellpadding=\"1\" cellspacing=\"0\" bgcolor=\"{}\"><tr><td>\
                 <table width=\"100%\" cellpadding=\"10\" cellspacing=\"0\" bgcolor=\"{}\"><tr><td>",
                c.line, c.card
            ),
        )
        .replace("</a></div>", "</a></td></tr></table></td></tr></table><br>");
    // Code blocks.
    body = body
        .replace(
            "<pre>",
            &format!(
                "<table width=\"100%\" cellpadding=\"8\" bgcolor=\"{}\"><tr><td><pre>",
                c.code
            ),
        )
        .replace("</pre>", "</pre></td></tr></table>");
    format!(
        "<html><head><title>{title}</title></head>\
         <body bgcolor=\"{paper}\" text=\"{ink}\" link=\"{accent}\">\
         <table width=\"100%\" cellpadding=\"6\" cellspacing=\"0\" bgcolor=\"{card}\"><tr><td>\
         &nbsp;&nbsp;<a href=\"reviera:help/index\">Home</a>{back}&nbsp;&nbsp;&nbsp;<font color=\"{muted}\">{title}</font>\
         </td></tr></table>\
         <table width=\"100%\" cellpadding=\"20\" cellspacing=\"0\"><tr><td>{body}</td></tr></table>\
         </body></html>",
        paper = c.paper,
        ink = c.ink,
        accent = c.accent,
        card = c.card,
        muted = c.muted,
    )
}

/// What a help link asks for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Link {
    Sample(String),
    Action(String),
    /// Another help page (`reviera:help/<slug>#<anchor>`).
    Help(Place),
    /// The help page shown before.
    HelpBack,
    External(String),
    /// Within the page (anchors), or the page itself.
    Page,
}

/// What a navigation to `url` is.
pub(crate) fn classify(url: &str) -> Link {
    if let Some(name) = url.strip_prefix("reviera:sample/") {
        return Link::Sample(name.to_owned());
    }
    if let Some(action) = url.strip_prefix("reviera:action/") {
        return Link::Action(action.to_owned());
    }
    if url == "reviera:help-back" {
        return Link::HelpBack;
    }
    if let Some(rest) = url.strip_prefix("reviera:help/") {
        let (slug, anchor) = match rest.split_once('#') {
            Some((s, a)) => (s, Some(a.to_owned()).filter(|a| !a.is_empty())),
            None => (rest, None),
        };
        let slug = if slug.is_empty() { "index" } else { slug };
        return Link::Help(Place {
            slug: slug.to_owned(),
            anchor,
        });
    }
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
        return Link::External(url.to_owned());
    }
    Link::Page
}

/// The folder of sample models: `REVIERA_SAMPLES`, beside the program (`samples`, or
/// `../share/reviera/samples`), or the source tree's `test-data/samples` in a development build.
pub(crate) fn samples_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("REVIERA_SAMPLES") {
        candidates.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("samples"));
        candidates.push(dir.join("../share/reviera/samples"));
        candidates.push(dir.join("../Resources/samples")); // macOS: Contents/MacOS/reviera
        // target/debug/reviera in the source tree.
        candidates.push(dir.join("../../test-data/samples"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples"));
    candidates
        .into_iter()
        .find(|d| d.join("samples.json").is_file())
}

/// A sample model: its name (file stem), title and one-line description.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sample {
    pub name: String,
    pub title: String,
    pub description: String,
}

/// The samples listed in `dir`'s `samples.json` whose `.ifc` exists.
pub(crate) fn samples(dir: &Path) -> Vec<Sample> {
    let Ok(text) = std::fs::read_to_string(dir.join("samples.json")) else {
        return Vec::new();
    };
    let Ok(list) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let name = s["name"].as_str()?.to_owned();
            if !dir.join(format!("{name}.ifc")).is_file() {
                return None;
            }
            let title = s["title"].as_str().unwrap_or(&name).to_owned();
            // The script's `-- description:` line.
            let description = std::fs::read_to_string(dir.join(format!("{name}.lua")))
                .ok()
                .and_then(|lua| {
                    lua.lines().find_map(|l| {
                        l.strip_prefix("-- description:")
                            .map(|d| d.trim().to_owned())
                    })
                })
                .unwrap_or_default();
            Some(Sample {
                name,
                title,
                description,
            })
        })
        .collect()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The landing page's samples section: a card for each sample found, or a note where there are
/// none.
fn sample_cards(samples: &[Sample]) -> String {
    if samples.is_empty() {
        return "<p class=\"note\">The sample models were not found beside Reviera (set \
         <code>REVIERA_SAMPLES</code> to their folder).</p>"
            .to_owned();
    }
    let mut out = String::from("<div class=\"samples\">\n");
    for s in samples {
        out.push_str(&format!(
            "<div class=\"sample\"><h3>{}</h3><p>{}</p><a class=\"open\" href=\"reviera:sample/{}\">Open</a></div>\n",
            escape(&s.title),
            escape(&s.description),
            escape(&s.name)
        ));
    }
    out.push_str("</div>");
    out
}

/// The landing page's contents: every other page, by group.
fn contents() -> String {
    let mut out = String::from("<div class=\"contents\">\n");
    let mut group = None;
    for p in PAGES.iter().filter(|p| p.group.is_some()) {
        if p.group != group {
            if group.is_some() {
                out.push_str("</ul></div>\n");
            }
            group = p.group;
            out.push_str(&format!(
                "<div><h3>{}</h3><ul>\n",
                escape(p.group.unwrap_or_default())
            ));
        }
        out.push_str(&format!(
            "<li><a href=\"reviera:help/{}\">{}</a></li>\n",
            p.slug,
            escape(p.title)
        ));
    }
    if group.is_some() {
        out.push_str("</ul></div>\n");
    }
    out.push_str("</div>");
    out
}

/// An anchor kept to the characters of an HTML id, so it is safe in the page's script.
fn clean_anchor(anchor: &str) -> String {
    anchor
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

/// The HTML of the page at `place` (`None` when there is no such page): its body in the frame,
/// with a Back link when `back` (there is a page to go back to). The landing page lists
/// `samples`.
pub(crate) fn page_html(place: &Place, samples: &[Sample], back: bool) -> Option<String> {
    let p = page(&place.slug)?;
    let body = if p.slug == "index" {
        p.body
            .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
            .replace("{{SAMPLES}}", &sample_cards(samples))
            .replace("{{CONTENTS}}", &contents())
    } else {
        p.body.to_owned()
    };
    let back = if back {
        "<a href=\"reviera:help-back\">Back</a>"
    } else {
        ""
    };
    let anchor = place
        .anchor
        .as_deref()
        .map(clean_anchor)
        .unwrap_or_default();
    // The body goes in last, so nothing in it is taken for a placeholder.
    Some(
        SHELL
            .replace("{{TITLE}}", &escape(p.title))
            .replace("{{BACK}}", back)
            .replace("{{ANCHOR}}", &anchor)
            .replace("{{BODY}}", &body),
    )
}

/// The landing page's HTML, with the samples found (or a note where there are none).
pub(crate) fn landing_html(samples: &[Sample]) -> String {
    page_html(&Place::home(), samples, false).expect("the landing page")
}

impl Gui {
    /// Help › Reviera Help: opens the help tab at its landing page, or shows it.
    pub(crate) fn show_help(&mut self) {
        if let Some(h) = &self.help
            && let Some(i) = super::wx::page_index(&self.notebook, &h.panel)
        {
            self.notebook.set_selection(i);
            return;
        }
        let panel = Panel::builder(&self.notebook).build();
        let frame = self.frame;
        let gui = self.this.clone();
        let follow = move |href: String| {
            let Some(gui) = gui.upgrade() else { return };
            match classify(&href) {
                Link::Sample(name) => with(&gui, |g| g.open_sample(&name)),
                Link::Action(action) => super::perform(&gui, &frame, &action),
                Link::Help(place) => with(&gui, |g| g.help_go(place)),
                Link::HelpBack => with(&gui, Gui::help_back),
                Link::External(url) => {
                    launch_default_browser(&url, BrowserLaunchFlags::Default);
                }
                Link::Page => {}
            }
        };
        let view = if needs_fallback() {
            HelpView::Html(super::wx::HtmlView::new(&panel, follow))
        } else {
            let view = WebView::builder(&panel).build();
            let sizer = BoxSizer::builder(Orientation::Vertical).build();
            sizer.add(&view, 1, SizerFlag::Expand, 0);
            panel.set_sizer(sizer, true);
            // The page's script puts a clicked link in its title (`link <n> <href>`): the
            // webview's own navigation events do not carry the URL through wxDragon.
            let v = view;
            view.on_title_changed(move |_| {
                let title = v.get_current_title();
                if let Some((_, href)) = title
                    .strip_prefix("link ")
                    .and_then(|rest| rest.split_once(' '))
                {
                    follow(href.to_owned());
                }
            });
            HelpView::Web(view)
        };
        self.notebook.add_page(&panel, "Help", true);
        self.help = Some(HelpTab {
            panel,
            view,
            here: Place::home(),
            back: Vec::new(),
        });
        self.help_show_page();
    }

    /// Shows the help page at `place` in the help tab, remembering the page shown for Back.
    pub(crate) fn help_go(&mut self, place: Place) {
        let Some(h) = &mut self.help else { return };
        if page(&place.slug).is_none() {
            self.frame
                .set_status_text(&format!("There is no help page {}", place.slug), 0);
            return;
        }
        let before = std::mem::replace(&mut h.here, place);
        h.back.push(before);
        if h.back.len() > BACK_LIMIT {
            h.back.remove(0);
        }
        self.help_show_page();
    }

    /// Goes back to the help page shown before.
    pub(crate) fn help_back(&mut self) {
        let Some(h) = &mut self.help else { return };
        let Some(place) = h.back.pop() else { return };
        h.here = place;
        self.help_show_page();
    }

    /// Loads the help tab's current page into its webview.
    fn help_show_page(&self) {
        let Some(h) = &self.help else { return };
        let samples = if h.here.slug == "index" {
            samples_dir().map(|d| samples(&d)).unwrap_or_default()
        } else {
            Vec::new()
        };
        let html = page_html(&h.here, &samples, !h.back.is_empty())
            .unwrap_or_else(|| landing_html(&samples));
        match h.view {
            HelpView::Web(v) => v.set_page(&html, "about:blank"),
            HelpView::Html(v) => {
                let anchor = h
                    .here
                    .anchor
                    .as_deref()
                    .map(clean_anchor)
                    .unwrap_or_default();
                v.set_page(
                    &plain_html(&html, wxdragon::appearance::is_system_dark_mode()),
                    &anchor,
                );
            }
        }
    }

    /// Help › About: the version, licence and main components.
    pub(crate) fn about(&mut self) {
        let text = format!(
            "NoDesk Reviera {}\n\nOpen-source building information modelling on IFC.\n\
             Licensed under the GNU GPL v3 or later.\n\n\
             IFC through IfcOpenShell; geometry through Open CASCADE; the window through \
             wxWidgets (wxDragon); drawing through wgpu; scripting through LuaJIT.",
            env!("CARGO_PKG_VERSION")
        );
        let dialog = MessageDialog::builder(&self.frame, &text, "About NoDesk Reviera")
            .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
            .build();
        dialog.show_modal();
    }

    /// Closes the help tab shown in notebook page `index`; returns whether it was it.
    pub(crate) fn close_help(&mut self, index: usize) -> bool {
        let is_help = self
            .help
            .as_ref()
            .and_then(|h| super::wx::page_index(&self.notebook, &h.panel))
            == Some(index);
        if is_help {
            self.help = None;
            self.notebook.delete_page(index);
        }
        is_help
    }

    /// Opens a fresh copy of sample `name` (in the system's temporary folder), so the sample
    /// stays as shipped. A copy already open is shown.
    pub(crate) fn open_sample(&mut self, name: &str) {
        let frame = self.frame;
        let Some(dir) = samples_dir() else {
            return error(&frame, "The sample models were not found");
        };
        let source = dir.join(format!("{name}.ifc"));
        let copies = std::env::temp_dir().join("reviera-samples");
        let target = copies.join(format!("{name}.ifc"));
        if self.app.document_at(&target).is_none() {
            let copied =
                std::fs::create_dir_all(&copies).and_then(|()| std::fs::copy(&source, &target));
            if let Err(e) = copied {
                return error(
                    &frame,
                    &format!("Could not copy the sample {}:\n{e}", source.display()),
                );
            }
        }
        self.open(&target, &frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_the_landing_page() {
        assert_eq!(
            classify("reviera:sample/manor"),
            Link::Sample("manor".into())
        );
        assert_eq!(
            classify("reviera:action/file.open"),
            Link::Action("file.open".into())
        );
        assert_eq!(
            classify("https://example.org"),
            Link::External("https://example.org".into())
        );
        assert_eq!(classify("about:blank#samples"), Link::Page);
        // Every sample in the source tree is listed, with its description, and links to open it.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples");
        let list = samples(&dir);
        let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
        for n in ["house", "observatory", "csg", "manor", "storefront"] {
            assert!(names.contains(&n), "{n} in {names:?}");
        }
        assert!(list.iter().all(|s| !s.description.is_empty()));
        let html = landing_html(&list);
        assert!(html.contains("href=\"reviera:sample/storefront\""));
        assert!(!html.contains("{{"));
    }

    /// The `href`s of `html`'s links.
    fn hrefs(html: &str) -> Vec<&str> {
        html.split("href=\"")
            .skip(1)
            .filter_map(|rest| rest.split_once('"').map(|(href, _)| href))
            .collect()
    }

    #[test]
    fn help_links() {
        assert_eq!(
            classify("reviera:help/walls"),
            Link::Help(Place {
                slug: "walls".into(),
                anchor: None
            })
        );
        assert_eq!(
            classify("reviera:help/walls#joins"),
            Link::Help(Place {
                slug: "walls".into(),
                anchor: Some("joins".into())
            })
        );
        assert_eq!(classify("reviera:help/"), Link::Help(Place::home()));
        assert_eq!(classify("reviera:help-back"), Link::HelpBack);
        assert_eq!(clean_anchor("a-b_c\"';x"), "a-b_cx");
    }

    #[test]
    fn every_page() {
        // The actions of the ribbon catalog.
        let catalog: toml_edit::DocumentMut = crate::ribbon::DEFAULT.parse().expect("the catalog");
        let actions: Vec<&str> = catalog["action"]
            .as_array_of_tables()
            .expect("actions")
            .iter()
            .filter_map(|a| a.get("id")?.as_str())
            .collect();
        assert!(actions.contains(&"wall.create"));
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/samples");
        let list = samples(&dir);
        let mut slugs = std::collections::HashSet::new();
        for p in PAGES {
            assert!(slugs.insert(p.slug), "{} twice", p.slug);
        }
        assert_eq!(PAGES[0].slug, "index");
        let index = landing_html(&list);
        for p in PAGES {
            let place = Place {
                slug: p.slug.into(),
                anchor: Some("x".into()),
            };
            let html = page_html(&place, &list, true).expect("the page");
            assert!(!html.contains("{{"), "{}: a placeholder is left", p.slug);
            assert_eq!(html.matches("<h1>").count(), 1, "{}: one <h1>", p.slug);
            assert!(html.contains("href=\"reviera:help-back\""));
            for tag in ["table", "ol", "ul", "pre", "h2", "h3", "main"] {
                let open = html.matches(&format!("<{tag}")).count();
                let close = html.matches(&format!("</{tag}>")).count();
                assert_eq!(
                    open, close,
                    "{}: <{tag}> opened {open}, closed {close}",
                    p.slug
                );
            }
            for href in hrefs(&html) {
                match classify(href) {
                    Link::Help(to) => {
                        let target = page(&to.slug)
                            .unwrap_or_else(|| panic!("{}: no page {}", p.slug, to.slug));
                        if let Some(a) = &to.anchor {
                            let body = page_html(
                                &Place {
                                    slug: target.slug.into(),
                                    anchor: None,
                                },
                                &list,
                                false,
                            )
                            .expect("the page");
                            assert!(
                                body.contains(&format!("id=\"{a}\"")),
                                "{}: no #{a} in {}",
                                p.slug,
                                to.slug
                            );
                        }
                    }
                    Link::Action(id) => {
                        assert!(actions.contains(&id.as_str()), "{}: no action {id}", p.slug)
                    }
                    Link::Sample(name) => assert!(
                        list.iter().any(|s| s.name == name),
                        "{}: no sample {name}",
                        p.slug
                    ),
                    Link::Page => {
                        if let Some(a) = href.strip_prefix('#') {
                            assert!(html.contains(&format!("id=\"{a}\"")), "{}: no #{a}", p.slug);
                        } else {
                            panic!("{}: link {href} goes nowhere", p.slug);
                        }
                    }
                    Link::HelpBack | Link::External(_) => {}
                }
            }
            // Every page but the landing page is in the contents.
            if p.slug != "index" {
                assert!(index.contains(&format!("href=\"reviera:help/{}\"", p.slug)));
            }
        }
    }
}
