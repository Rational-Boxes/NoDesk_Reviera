// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Remote Analysis (DM-RPC-011): the subscriptions to remote analysis services, with their
//! status, plan, usage and period; Add…, Edit…, Remove, Test Connection, Refresh and Set as
//! Default. Add asks for a name, the service URL and the API key and secret (masked, never shown
//! again), with Advanced settings for a self-hosted service (an extra CA, a certificate pin,
//! development use of `http://localhost`), and tests the connection before saving.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const ADD: i32 = 7101;
const EDIT: i32 = 7102;
const REMOVE: i32 = 7103;
const TEST: i32 = 7104;
const REFRESH: i32 = 7105;
const DEFAULT: i32 = 7106;

fn state_label(s: &str) -> &str {
    match s {
        "active" => "Active",
        "trial" => "Trial",
        "suspended" => "Suspended",
        "expired" => "Expired",
        "refused" => "Key refused",
        "unreachable" => "Unreachable",
        "secret_missing" => "Secret needed",
        "not_checked" => "Not checked",
        other => other,
    }
}

fn unit_label(u: &str) -> String {
    u.replace('_', " ")
}

fn usage(st: &Value) -> String {
    let (a, u, unit) = (st["allowance"].as_f64().unwrap_or(0.0), st["usage"].as_f64().unwrap_or(0.0), unit_label(st["unit"].as_str().unwrap_or("")));
    if a > 0.0 {
        format!("{u} of {a} {unit}")
    } else if u > 0.0 {
        format!("{u} {unit} (unlimited)")
    } else {
        String::new()
    }
}

fn date(s: &str) -> &str {
    s.get(..10).unwrap_or(s)
}

/// What the dialog shows under the list for one subscription.
fn detail(s: &Value) -> String {
    let st = &s["status"];
    let mut lines = Vec::new();
    if let Some(m) = st["message"].as_str() {
        lines.push(m.to_owned());
    }
    for w in st["warnings"].as_array().into_iter().flatten() {
        lines.push(format!("⚠ {}", w.as_str().unwrap_or("")));
    }
    if let Some(r) = st["remaining"].as_f64() {
        lines.push(format!("Remaining this period: {r} {}", unit_label(st["unit"].as_str().unwrap_or(""))));
    }
    let kinds: Vec<&str> = st["job_kinds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !kinds.is_empty() {
        lines.push(format!("Job kinds: {}", kinds.join(", ")));
    }
    let mut limits = Vec::new();
    if let Some(n) = st["concurrent_jobs"].as_u64().filter(|n| *n > 0) {
        limits.push(format!("{n} jobs at once"));
    }
    if let Some(b) = st["max_job_bytes"].as_u64().filter(|n| *n > 0) {
        limits.push(format!("jobs up to {:.1} GB", b as f64 / 1e9));
    }
    if !limits.is_empty() {
        lines.push(format!("Limits: {}", limits.join(", ")));
    }
    let defaults: Vec<&str> = s["default_for"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !defaults.is_empty() {
        lines.push(format!("Default for: {}", defaults.join(", ")));
    }
    lines.push(
        match s["secret_storage"].as_str() {
            Some("keychain") => "The secret is in the system keychain.",
            Some("session") => "No keychain is available: the secret is kept until Reviera closes.",
            _ => "The secret is not stored: Edit… takes it again.",
        }
        .to_owned(),
    );
    if let Some(t) = st["checked_at"].as_str() {
        lines.push(format!("Last checked {} UTC", t.replace('T', " ").trim_end_matches('Z')));
    }
    if let Some(t) = st["terms_url"].as_str().filter(|t| !t.is_empty()) {
        lines.push(format!("Terms: {t}"));
    }
    lines.join("\n")
}

/// The list dialog: which button closed it, and the selected row.
fn list_dialog(frame: &Frame, subs: &[Value], selected: Option<&str>, note: &str) -> (i32, Option<usize>) {
    let dialog = Dialog::builder(frame, "Remote Analysis").with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
    let intro = StaticText::builder(&dialog)
        .with_label(
            "Subscriptions to remote analysis services. Reviera connects to a service only after you add it, \
             and sends the key and secret only to that service's https:// address.",
        )
        .build();
    intro.wrap(720);
    let list = ListCtrl::builder(&dialog).with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel).build();
    for (k, (h, w)) in [("Name", 150), ("Service URL", 230), ("Status", 100), ("Plan", 110), ("Usage", 170), ("Period ends", 95), ("Default", 60)]
        .into_iter()
        .enumerate()
    {
        list.insert_column(k as i64, h, ListColumnFormat::Left, w);
    }
    for (k, s) in subs.iter().enumerate() {
        let st = &s["status"];
        list.insert_item(k as i64, s["name"].as_str().unwrap_or(""), None);
        let cells = [
            s["url"].as_str().unwrap_or("").to_owned(),
            state_label(st["state"].as_str().unwrap_or("")).to_owned(),
            st["plan"].as_str().unwrap_or("").to_owned(),
            usage(st),
            date(st["period_end"].as_str().unwrap_or("")).to_owned(),
            if s["default"].as_bool() == Some(true) { "Yes".into() } else { String::new() },
        ];
        for (c, text) in cells.iter().enumerate() {
            list.set_item_text_by_column(k as i64, c as i32 + 1, text);
        }
        let warn = st["warnings"].as_array().is_some_and(|w| !w.is_empty());
        let colour = match st["state"].as_str() {
            Some("active" | "trial") if !warn => None,
            Some("active" | "trial") => Some(Colour::new(170, 110, 0, 255)),
            Some("not_checked") => None,
            _ => Some(Colour::new(200, 30, 30, 255)),
        };
        if let Some(c) = colour {
            list.set_item_text_colour(k as i64, &c);
        }
    }
    let info = StaticText::builder(&dialog)
        .with_label(if subs.is_empty() { "There are no subscriptions. Add… asks for the service URL and the API key and secret." } else { "" })
        .build();
    info.set_min_size(Size::new(-1, 190));
    let at = selected.and_then(|id| subs.iter().position(|s| s["id"].as_str() == Some(id))).or((!subs.is_empty()).then_some(0));
    if let Some(k) = at {
        list.set_item_state(k as i64, ListItemState::Selected, ListItemState::Selected);
        info.set_label(&detail(&subs[k]));
    }
    let rows = subs.to_vec();
    let (i2, d2) = (info, dialog);
    list.on_item_selected(move |e| {
        if let Some(s) = usize::try_from(e.get_item_index()).ok().and_then(|k| rows.get(k)) {
            i2.set_label(&detail(s));
            i2.wrap(720);
            d2.layout();
        }
    });
    let d3 = dialog;
    list.on_item_activated(move |_| d3.end_modal(EDIT));

    let buttons = BoxSizer::builder(Orientation::Vertical).build();
    for (code, label, tip) in [
        (ADD, "Add…", "Add a subscription: its name, service URL, API key and secret"),
        (EDIT, "Edit…", "Change the name, URL, key, secret or advanced settings"),
        (REMOVE, "Remove", "Remove the subscription and delete its secret from the keychain"),
        (TEST, "Test Connection", "Sign in afresh and ask the service for the subscription's status"),
        (REFRESH, "Refresh", "Ask the service for the subscription's current status"),
        (DEFAULT, "Set as Default", "Use this subscription by default, for every job kind or one"),
    ] {
        let b = Button::builder(&dialog).with_label(label).build();
        b.set_tooltip(tip);
        b.enable(code == ADD || !subs.is_empty());
        let d = dialog;
        b.on_click(move |_| d.end_modal(code));
        buttons.add(&b, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
    }
    buttons.add_stretch_spacer(1);
    let close = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
    buttons.add(&close, 0, SizerFlag::Expand, 0);

    let left = BoxSizer::builder(Orientation::Vertical).build();
    left.add(&intro, 0, SizerFlag::Expand | SizerFlag::Bottom, 8);
    left.add(&list, 1, SizerFlag::Expand | SizerFlag::Bottom, 8);
    if !note.is_empty() {
        let n = StaticText::builder(&dialog).with_label(note).build();
        n.wrap(720);
        left.add(&n, 0, SizerFlag::Expand | SizerFlag::Bottom, 6);
    }
    left.add(&info, 0, SizerFlag::Expand, 0);
    let sizer = BoxSizer::builder(Orientation::Horizontal).build();
    sizer.add_sizer(&left, 1, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom | SizerFlag::Right, 12);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_escape_id(ID_CANCEL);
    let code = dialog.show_modal();
    let sel = usize::try_from(list.get_first_selected_item()).ok();
    dialog.destroy();
    (code, sel)
}

/// What the Add or Edit form holds between attempts. The secret is never filled from storage.
#[derive(Default)]
struct Fields {
    name: String,
    url: String,
    key: String,
    secret: String,
    ca: String,
    remove_ca: bool,
    pin: String,
    development: bool,
}

impl Gui {
    /// Analyze › Remote Analysis (and Manage › Settings): the subscriptions.
    pub(crate) fn remote_analysis(&mut self) {
        let frame = self.frame;
        let (mut selected, mut note): (Option<String>, String) = (None, String::new());
        loop {
            let subs = match self.run("subscription.list", Value::Null) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &format!("The subscriptions could not be read:\n{e}")),
            };
            let (code, at) = list_dialog(&frame, &subs, selected.as_deref(), &note);
            note.clear();
            let chosen = at.and_then(|k| subs.get(k)).cloned();
            selected = chosen.as_ref().and_then(|s| s["id"].as_str().map(str::to_owned));
            let id = selected.clone().unwrap_or_default();
            let name = chosen.as_ref().and_then(|s| s["name"].as_str()).unwrap_or("").to_owned();
            match (code, &chosen) {
                (ADD, _) => {
                    if let Some(new) = self.subscription_form(None) {
                        selected = Some(new);
                    }
                }
                (EDIT, Some(s)) => {
                    self.subscription_form(Some(s));
                }
                (REMOVE, Some(_)) => {
                    let d = MessageDialog::builder(
                        &frame,
                        &format!("Remove the subscription {name}? Its secret is deleted from the keychain. Jobs already on the service keep running there, but can only be fetched after it is added again."),
                        "Remote Analysis",
                    )
                    .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                    .build();
                    if d.show_modal() == ID_YES {
                        if let Err(e) = self.run("subscription.remove", json!({ "subscription_id": id })) {
                            error(&frame, &format!("The subscription was not removed:\n{e}"));
                        }
                        selected = None;
                    }
                }
                (TEST | REFRESH, Some(_)) => {
                    frame.set_status_text(&format!("Connecting to {name}…"), 0);
                    let r = if code == TEST {
                        self.run("subscription.test", json!({ "subscription_id": id }))
                    } else {
                        self.run("subscription.status", json!({ "subscription_id": id, "refresh": true }))
                    };
                    note = match r {
                        Ok(st) => match st["message"].as_str() {
                            Some(m) if st["reason"].is_string() => format!("{name}: {m}"),
                            _ if code == REFRESH && st["state"] == "refused" => {
                                format!("{name}: the key was refused, so it is not tried again until it is changed (Edit…) or tested")
                            }
                            _ => format!("{name}: connected; {} — {}", state_label(st["state"].as_str().unwrap_or("")), st["plan"].as_str().unwrap_or("")),
                        },
                        Err(e) => format!("{name}: {e}"),
                    };
                    frame.set_status_text("", 0);
                }
                (DEFAULT, Some(s)) => {
                    let mut kinds = vec!["Every job kind".to_owned()];
                    kinds.extend(s["status"]["job_kinds"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
                    let refs: Vec<&str> = kinds.iter().map(String::as_str).collect();
                    let d = SingleChoiceDialog::builder(&frame, &format!("Use {name} by default for:"), "Set as Default", &refs).build();
                    if d.show_modal() == ID_OK {
                        let k = d.get_selection();
                        let mut args = json!({ "subscription_id": id });
                        if k > 0 {
                            args["job_kind"] = json!(kinds[k as usize]);
                        }
                        if let Err(e) = self.run("subscription.set_default", args) {
                            error(&frame, &e);
                        }
                    }
                }
                (EDIT | REMOVE | TEST | REFRESH | DEFAULT, None) => note = "Select a subscription first.".into(),
                _ => break,
            }
        }
    }

    /// Add… (`existing` is `None`) or Edit…: returns the subscription's id once saved. The form
    /// comes back with its values after a failed test, so nothing has to be typed again.
    fn subscription_form(&mut self, existing: Option<&Value>) -> Option<String> {
        let frame = self.frame;
        let mut v = match existing {
            Some(s) => Fields {
                name: s["name"].as_str().unwrap_or("").into(),
                url: s["url"].as_str().unwrap_or("").into(),
                key: s["key"].as_str().unwrap_or("").into(),
                pin: s["pin"].as_str().unwrap_or("").into(),
                development: s["development"].as_bool().unwrap_or(false),
                ..Fields::default()
            },
            None => Fields { url: "https://".into(), ..Fields::default() },
        };
        let has_ca = existing.is_some_and(|s| s["ca"].as_bool() == Some(true));
        let title = if existing.is_some() { "Edit Subscription" } else { "Add Subscription" };
        loop {
            let f = super::reinforcement_ui::Form::new(&frame, title);
            let name = f.text("Name", &v.name, "How the subscription is listed (for example the service's or the plan's name)");
            let url = f.text("Service URL", &v.url, "The service's secure address, https://…");
            let key = f.text("API key", &v.key, "The subscription's key; or key:secret in one, leaving Secret empty");
            let secret_tip = if existing.is_some() { "A new secret replaces the stored one; leave empty to keep it" } else { "The key's secret: kept in the system keychain, never shown again" };
            let secret = f.password("Secret", secret_tip);
            secret.set_value(&v.secret);
            let ca = f.text(
                "Advanced: CA file",
                &v.ca,
                if has_ca { "A PEM file of an extra CA to trust; empty keeps the current one" } else { "For a self-hosted service: a PEM file of the CA that signed its certificate" },
            );
            let remove_ca = has_ca.then(|| f.check("", "Remove the trusted CA", v.remove_ca));
            let pin = f.text("Advanced: pin", &v.pin, "For a self-hosted service: the SHA-256 of its certificate (64 hex digits); it replaces CA checks");
            let dev = f.check("Advanced", "Development: allow http://localhost", v.development);
            let note = if existing.is_some() {
                "Changes to the URL, key, secret or advanced settings are tested before they are saved."
            } else {
                "The connection is tested before the subscription is saved."
            };
            let ok = f.show(note, if existing.is_some() { "Save" } else { "Add" });
            v = Fields {
                name: name.get_value(),
                url: url.get_value(),
                key: key.get_value(),
                secret: secret.get_value(),
                ca: ca.get_value(),
                remove_ca: remove_ca.as_ref().is_some_and(CheckBox::get_value),
                pin: pin.get_value(),
                development: dev.get_value(),
            };
            f.dialog.destroy();
            if !ok {
                return None;
            }
            frame.set_status_text("Testing the connection…", 0);
            let r = match existing {
                None => {
                    let mut a = json!({ "name": v.name, "url": v.url, "key": v.key, "development": v.development });
                    if !v.secret.is_empty() {
                        a["secret"] = json!(v.secret);
                    }
                    if !v.ca.trim().is_empty() {
                        a["ca"] = json!(v.ca);
                    }
                    if !v.pin.trim().is_empty() {
                        a["pin"] = json!(v.pin);
                    }
                    self.run("subscription.add", a).map(|o| o["subscription_id"].as_str().unwrap_or("").to_owned())
                }
                Some(s) => {
                    let id = s["id"].as_str().unwrap_or("").to_owned();
                    let mut a = json!({ "subscription_id": id, "name": v.name, "url": v.url, "key": v.key, "pin": v.pin, "development": v.development });
                    if !v.secret.is_empty() {
                        a["secret"] = json!(v.secret);
                    }
                    if v.remove_ca {
                        a["ca"] = json!("");
                    } else if !v.ca.trim().is_empty() {
                        a["ca"] = json!(v.ca);
                    }
                    self.run("subscription.set", a).map(|_| id)
                }
            };
            frame.set_status_text("", 0);
            match r {
                Ok(id) => {
                    frame.set_status_text(&format!("Subscription {} saved", v.name), 0);
                    return Some(id);
                }
                Err(e) => error(&frame, &format!("The subscription was not saved:\n{e}")),
            }
        }
    }
}
