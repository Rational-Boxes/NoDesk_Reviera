// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Help (UI-HELP-001): a hypertext help page in a tab of the view notebook, beside the model's
//! views. Help › Reviera Help opens the landing page (or shows it when it is open). Its links:
//! - `reviera:sample/<name>` opens a fresh copy of a sample model (`test-data/samples`), so the
//!   sample stays as shipped;
//! - `reviera:action/<id>` runs an action, as the ribbon would;
//! - `#anchor` moves within the page; web links open in the system browser.
//!
//! A small script in the page catches those clicks and hands the link over through the page's
//! title, which works with every webview backend.
//!
//! The page is HTML compiled into the application; the samples section is built from the
//! samples folder's `samples.json` and each script's `-- description:` line.

use std::path::{Path, PathBuf};

use wxdragon::event::WebViewEvents;
use wxdragon::prelude::*;
use wxdragon::widgets::WebView;

use super::{Gui, error, with};

/// The landing page, with `{{VERSION}}` and `{{SAMPLES}}` to fill in.
const LANDING: &str = include_str!("../help/index.html");

/// The open help tab.
pub struct HelpTab {
    pub panel: Panel,
}

/// What a help link asks for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Link {
    Sample(String),
    Action(String),
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
                    lua.lines()
                        .find_map(|l| l.strip_prefix("-- description:").map(|d| d.trim().to_owned()))
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

/// The landing page's HTML, with the samples found (or a note where there are none).
pub(crate) fn landing_html(samples: &[Sample]) -> String {
    let cards = if samples.is_empty() {
        "<p class=\"note\">The sample models were not found beside Reviera (set \
         <code>REVIERA_SAMPLES</code> to their folder).</p>"
            .to_owned()
    } else {
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
    };
    LANDING
        .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
        .replace("{{SAMPLES}}", &cards)
}

impl Gui {
    /// Help › Reviera Help: opens the help tab, or shows it.
    pub(crate) fn show_help(&mut self) {
        if let Some(h) = &self.help
            && let Some(i) = super::wx::page_index(&self.notebook, &h.panel)
        {
            self.notebook.set_selection(i);
            return;
        }
        let panel = Panel::builder(&self.notebook).build();
        let view = WebView::builder(&panel).build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&view, 1, SizerFlag::Expand, 0);
        panel.set_sizer(sizer, true);
        let list = samples_dir().map(|d| samples(&d)).unwrap_or_default();
        view.set_page(&landing_html(&list), "about:blank");
        if let Some(gui) = self.this.upgrade() {
            let frame = self.frame;
            // The page's script puts a clicked link in its title (`link <n> <href>`): the
            // webview's own navigation events do not carry the URL through wxDragon.
            let v = view;
            view.on_title_changed(move |_| {
                let title = v.get_current_title();
                let Some(href) = title
                    .strip_prefix("link ")
                    .and_then(|rest| rest.split_once(' '))
                    .map(|(_, href)| href.to_owned())
                else {
                    return;
                };
                match classify(&href) {
                    Link::Sample(name) => with(&gui, |g| g.open_sample(&name)),
                    Link::Action(action) => super::perform(&gui, &frame, &action),
                    Link::External(url) => {
                        launch_default_browser(&url, BrowserLaunchFlags::Default);
                    }
                    Link::Page => {}
                }
            });
        }
        self.notebook.add_page(&panel, "Help", true);
        self.help = Some(HelpTab { panel });
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
            let copied = std::fs::create_dir_all(&copies).and_then(|()| std::fs::copy(&source, &target));
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
        assert_eq!(classify("reviera:sample/manor"), Link::Sample("manor".into()));
        assert_eq!(classify("reviera:action/file.open"), Link::Action("file.open".into()));
        assert_eq!(classify("https://example.org"), Link::External("https://example.org".into()));
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
}
