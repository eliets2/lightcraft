//! Headless tests of the editable keymap (Help ▸ Keyboard Shortcuts): a changed shortcut fires
//! its command and not the old one's, the editor records a key press without running it, Esc
//! cancels recording without closing the dialog, and the keymap survives a save/load of `ui.json`.

use std::time::Duration;

use serde_json::json;

use crate::headless::Headless;
use crate::state::{Dialog, ViewMode};
use crate::{LightcraftApp, Services};

const T: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(120);

fn demo() -> Headless {
    let services = Services { png: None, ..Default::default() };
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    let mut h = Headless::new(app, [1400.0, 900.0], 1.0);
    let r = h.request("ui.set", json!({"view": "detail"}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.settle(SETTLE);
    h
}

fn key(h: &mut Headless, k: &str, shift: bool) {
    let r = h.request("ui.key", json!({"key": k, "shift": shift}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.step();
}

fn click(h: &mut Headless, id: &str) {
    let r = h.request("ui.clickWidget", json!({"id": id}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.step();
}

#[test]
fn rebound_shortcut_fires_and_the_old_key_does_not() {
    let mut h = demo();
    let r = h.app.run("app.setShortcut", json!({"id": "view.survey", "shortcut": "Shift+K"})).unwrap();
    assert_eq!(r["shortcut"], "Shift+K", "{r}");
    // the old key (N) no longer opens Survey…
    key(&mut h, "N", false);
    assert_eq!(h.app.ui.view, ViewMode::Detail);
    // …the new one does, and menus show it
    key(&mut h, "K", true);
    assert_eq!(h.app.ui.view, ViewMode::Survey);
    let entry = crate::menus::menu_entries(&h.app).into_iter().find(|e| e.id == "view.survey").unwrap();
    assert_eq!(entry.shortcut.as_deref(), Some("Shift+K"));
    // the keymap is saved with the UI state
    let saved = serde_json::to_string(&h.app.ui).unwrap();
    let back = serde_json::from_str::<crate::UiState>(&saved).unwrap();
    assert_eq!(back.settings.keymap.get("view.survey").map(String::as_str), Some("Shift+K"));
}

#[test]
fn the_editor_records_a_key_and_esc_cancels_without_closing() {
    let mut h = demo();
    h.app.run("app.shortcuts", json!({})).unwrap();
    h.step();
    h.step();
    // the list is longer than the dialog: narrow it so the row is on screen
    click(&mut h, "field:shortcutsSearch");
    assert_eq!(h.request("ui.text", json!({"text": "compare"}), T)["ok"], true);
    h.step();
    h.step();
    // record Shift+J for Compare: the key press is taken, not run
    click(&mut h, "button:shortcut-view.compare");
    assert_eq!(h.app.recording_shortcut.as_deref(), Some("view.compare"));
    key(&mut h, "J", true);
    assert_eq!(h.app.recording_shortcut, None);
    assert_eq!(crate::shortcuts::shortcut_of(&h.app.ui.settings.keymap, "view.compare"), Some("Shift+J"));
    assert_eq!(h.app.ui.view, ViewMode::Detail);
    assert!(matches!(h.app.ui.dialog, Some(Dialog::Shortcuts)));
    // Esc while recording cancels; the dialog stays open and nothing changes
    click(&mut h, "button:shortcut-view.compare");
    key(&mut h, "Escape", false);
    assert_eq!(h.app.recording_shortcut, None);
    assert!(matches!(h.app.ui.dialog, Some(Dialog::Shortcuts)));
    assert_eq!(crate::shortcuts::shortcut_of(&h.app.ui.settings.keymap, "view.compare"), Some("Shift+J"));
    // the next Esc closes it; the new key works
    key(&mut h, "Escape", false);
    assert!(h.app.ui.dialog.is_none());
    key(&mut h, "J", true);
    assert_eq!(h.app.ui.view, ViewMode::Compare);
    // Reset All restores the declared keys
    h.app.run("app.resetShortcuts", json!({})).unwrap();
    assert_eq!(crate::shortcuts::shortcut_of(&h.app.ui.settings.keymap, "view.compare"), Some("Shift+C"));
}

#[test]
fn a_key_given_to_a_command_overrides_a_fixed_key() {
    let mut h = demo();
    let before = h.app.session.active().and_then(|id| h.app.session.catalog.photo(id)).map(|p| p.rating);
    // 3 (rate ★★★) now opens Survey instead
    h.app.run("app.setShortcut", json!({"id": "view.survey", "shortcut": "3"})).unwrap();
    key(&mut h, "3", false);
    assert_eq!(h.app.ui.view, ViewMode::Survey);
    let after = h.app.session.active().and_then(|id| h.app.session.catalog.photo(id)).map(|p| p.rating);
    assert_eq!(before, after, "the rating key must not fire too");
}

/// Open the editor, filtered to `search`, and start recording for `id`.
fn record(h: &mut Headless, search: &str, id: &str) {
    if h.app.ui.dialog.is_none() {
        h.app.run("app.shortcuts", json!({})).unwrap();
        h.step();
        h.step();
        click(h, "field:shortcutsSearch");
        assert_eq!(h.request("ui.text", json!({"text": search}), T)["ok"], true);
        h.step();
        h.step();
    }
    click(h, &format!("button:shortcut-{id}"));
    assert_eq!(h.app.recording_shortcut.as_deref(), Some(id));
}

/// Pressing ⌘ (a key event of its own) before S used to record `Cmd+SuperLeft`, so no ⌘ shortcut
/// could be set; ⌘C/⌘X/⌘V arrive as copy/cut/paste events, not keys, and were ignored.
#[test]
fn modifiers_wait_for_their_key_and_clipboard_keys_record() {
    let mut h = demo();
    record(&mut h, "survey", "view.survey");
    let r = h.request("ui.key", json!({"key": "SuperLeft", "cmd": true}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.step();
    assert_eq!(h.app.recording_shortcut.as_deref(), Some("view.survey"), "still waiting after ⌘ alone");
    let r = h.request("ui.key", json!({"key": "K", "cmd": true, "shift": true}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.step();
    assert_eq!(h.app.recording_shortcut, None);
    assert_eq!(crate::shortcuts::shortcut_of(&h.app.ui.settings.keymap, "view.survey"), Some("Cmd+Shift+K"));
    // ⌘⌥C: the windowing layer sends a Copy event with the modifiers held
    record(&mut h, "survey", "view.survey");
    let m = egui::Modifiers { alt: true, ..egui::Modifiers::COMMAND };
    h.app.synthetic.push(egui::Event::Key { key: egui::Key::AltLeft, physical_key: None, pressed: true, repeat: false, modifiers: m });
    h.app.synthetic.push(egui::Event::Copy);
    h.step();
    h.step();
    assert_eq!(crate::shortcuts::shortcut_of(&h.app.ui.settings.keymap, "view.survey"), Some("Cmd+Alt+C"));
    // a saved modifier-only shortcut (from the bug) means no shortcut
    assert_eq!(crate::shortcuts::parse("Cmd+SuperLeft"), None);
}

/// On Windows and Linux the windowing layer turns Ctrl+C / Ctrl+V into clipboard events, not key
/// presses: outside a text field they still copy the edit settings, and with ⇧ open Paste Selected
/// Settings.
#[test]
fn clipboard_events_run_the_copy_and_paste_shortcuts() {
    let mut h = demo();
    assert!(h.app.session.clipboard.is_none());
    h.events.push(egui::Event::Copy);
    h.settle(SETTLE);
    assert!(h.app.session.clipboard.is_some(), "Ctrl+C copied the edit settings");
    let shift_cmd = egui::Modifiers::SHIFT | egui::Modifiers::COMMAND;
    h.events.extend([egui::Event::ModifiersChanged(shift_cmd), egui::Event::Paste("text".into())]);
    h.settle(SETTLE);
    assert!(matches!(h.app.ui.dialog, Some(Dialog::PasteSettings { .. })), "⇧Ctrl+V: {:?}", h.app.ui.dialog);
}

/// Windows also pastes with ⇧Insert and cuts with ⇧Delete: the clipboard events they become are
/// the plain Paste / Cut, not ⇧Ctrl+V (Paste Selected Settings).
#[test]
fn shift_insert_pastes_plainly() {
    let mut h = demo();
    h.events.push(egui::Event::Copy);
    h.settle(SETTLE);
    let undo = h.app.session.undo.len();
    h.events.extend([egui::Event::ModifiersChanged(egui::Modifiers::SHIFT), egui::Event::Paste("text".into())]);
    h.settle(SETTLE);
    assert_eq!(h.app.ui.dialog, None, "no Paste Selected Settings");
    assert_eq!(h.app.session.undo.len(), undo + 1, "the settings were pasted");
}
/// The opening release can carry both physical shortcut keys and the replacement text.
#[test]
fn opening_numeric_release_keeps_keys_in_the_editor() {
    use egui::{Event, Key, Modifiers, PointerButton};

    for terminal in [Key::Enter, Key::Escape] {
        let mut h = demo();
        h.app.run("photo.rate", json!({"rating": 0})).unwrap();
        h.app.run("develop.set", json!({"values": {"light.exposure": 0.7}})).unwrap();
        h.app.run("app.setShortcut", json!({"id": "photo.increaseRating", "shortcut": "Enter"})).unwrap();
        let r = h.request("ui.set", json!({"view": "detail", "right": "edit", "openSections": ["light"], "histogram": false}), T);
        assert_eq!(r["ok"], true, "{r}");
        assert!(h.settle(SETTLE));
        assert!(h.app.fonts_ready);
        assert!(h.app.recording_shortcut.is_none() && h.app.ui.name_edit.is_none());
        assert!(h.app.native_shortcuts.is_empty() && h.app.synthetic.is_empty() && h.events.is_empty());
        assert!(!h.view.ctx.egui_wants_keyboard_input());
        assert_eq!(crate::shortcuts::shortcut_of(&h.app.ui.settings.keymap, "photo.increaseRating"), Some("Enter"));
        let active = h.app.session.active().unwrap();
        let selection = h.app.session.selection.ids.clone();
        let (view, right, tool) = (h.app.ui.view, h.app.ui.right, h.app.ui.tool.clone());
        assert!(h.app.ui.dialog.is_none());
        assert_eq!(h.app.session.catalog.photo(active).unwrap().rating, 0);
        assert!((h.app.session.develop_of(active).unwrap().light.exposure - 0.7).abs() < 1e-9);
        let (undo, redo) = (h.app.session.undo.len(), h.app.session.redo.len());
        let pos = h.app.widgets.iter().rev().find(|(id, _)| id == "sliderValue:light.exposure").unwrap().1.center();
        h.events.push(Event::PointerMoved(pos));
        h.step();
        h.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.step();
        assert!(h.view.ctx.egui_wants_keyboard_input(), "the press focuses the editor before app shortcuts run");
        h.events.extend([
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
            Event::Key { key: Key::Num2, physical_key: Some(Key::Num2), pressed: true, repeat: false, modifiers: Modifiers::NONE },
            Event::Text("2".into()),
            Event::Key { key: terminal, physical_key: Some(terminal), pressed: true, repeat: false, modifiers: Modifiers::NONE },
        ]);
        h.step();
        // Observe the consuming frame directly, then exactly one empty frame: no request drains.
        for next_frame in [false, true] {
            if next_frame {
                assert!(h.events.is_empty() && h.app.synthetic.is_empty());
                h.step();
            }
            let expected = if terminal == Key::Enter { 2.0 } else { 0.7 };
            assert!((h.app.session.develop_of(active).unwrap().light.exposure - expected).abs() < 1e-9, "{terminal:?}, next={next_frame}");
            assert_eq!(h.app.session.undo.len(), undo + usize::from(terminal == Key::Enter), "{terminal:?}, next={next_frame}");
            assert_eq!(h.app.session.redo.len(), redo);
            assert_eq!(h.app.session.catalog.photo(active).unwrap().rating, 0, "neither Num2 nor the saved Enter shortcut leaked");
            assert_eq!(h.app.session.active(), Some(active));
            assert_eq!(h.app.session.selection.ids, selection);
            assert_eq!((h.app.ui.view, h.app.ui.right, &h.app.ui.tool), (view, right, &tool));
            assert!(h.app.ui.dialog.is_none());
            assert!(!h.view.ctx.egui_wants_keyboard_input());
            if next_frame {
                assert!(h.app.widgets.iter().all(|(id, _)| id != "sliderField:light.exposure"), "the editor is gone");
                assert!(h.app.widgets.iter().any(|(id, _)| id == "sliderValue:light.exposure"));
            }
        }
    }
}

/// Press `key` with the named modifiers held (`cmd`, `shift`, `alt`, `ctrl`).
fn press(h: &mut Headless, key: &str, mods: &[&str]) {
    let mut p = json!({ "key": key });
    for m in mods {
        p[*m] = json!(true);
    }
    let r = h.request("ui.key", p, T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
}

fn view(h: &mut Headless, p: serde_json::Value) {
    let r = h.request("ui.set", p, T);
    assert_eq!(r["ok"], true, "{r}");
}

/// The declared shortcut of every command these tests bind, and the shape of each alias: a key
/// that doesn't parse would never fire.
#[test]
fn parity_keys_are_declared_and_parse() {
    use crate::shortcuts::{ALIASES, Keymap, parse, shortcut_of};

    let keymap = Keymap::new();
    for (id, want) in [
        ("view.visualizeHdr", "Shift+H"),
        ("app.openLibrary", "Cmd+O"),
        ("library.findSimilar", "Cmd+Alt+Shift+F"),
        ("develop.matchExposure", "Cmd+Alt+Shift+M"),
    ] {
        assert_eq!(shortcut_of(&keymap, id), Some(want), "{id}");
        assert!(parse(want).is_some(), "{want} does not parse");
    }
    for (sc, id) in [
        ("Cmd+Alt+Shift+Left", "beforeAfter.copyAfterToBefore"),
        ("Cmd+Alt+Shift+Right", "beforeAfter.copyBeforeToAfter"),
        ("Cmd+Alt+Shift+Up", "beforeAfter.swap"),
        ("Cmd+Enter", "view.slideshow"),
    ] {
        assert!(ALIASES.iter().any(|(s, i, _)| *s == sc && *i == id), "{sc} → {id}");
        assert!(parse(sc).is_some(), "{sc} does not parse");
    }
}

/// Opening a library uses the same folder picker as File > Open Library. Cancelling it
/// leaves the current photo and library intact.
#[test]
fn cmd_o_opens_the_library_picker_once() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut h = demo();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    h.app.services.pick_folder = Some(Box::new(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        None
    }));
    let active = h.app.session.active();
    let photos = h.app.session.catalog.len();
    press(&mut h, "O", &["cmd"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one picker for Cmd+O");
    assert_eq!(h.app.session.active(), active);
    assert_eq!(h.app.session.catalog.len(), photos);
}

/// ⇧H toggles View ▸ Visualize HDR; ⌘⇧H (Histogram) is untouched.
#[test]
fn shift_h_toggles_the_hdr_view() {
    let mut h = demo();
    assert!(!h.app.ui.hdr_visualize);
    let histogram = h.app.ui.histogram;
    press(&mut h, "H", &["shift"]);
    assert!(h.app.ui.hdr_visualize, "⇧H turns Visualize HDR on");
    assert_eq!(h.app.ui.histogram, histogram, "⇧H leaves the Histogram unchanged");
    press(&mut h, "H", &["shift"]);
    assert!(!h.app.ui.hdr_visualize, "⇧H turns it back off");
}

/// ⌘↩ plays the photos in view as the impromptu slideshow (the declared key stays ⌘⌥↩).
#[test]
fn cmd_enter_plays_the_slideshow() {
    let mut h = demo();
    assert_eq!(h.app.ui.slideshow, None);
    let panel = h.app.ui.right;
    press(&mut h, "Enter", &["cmd"]);
    assert!(h.app.ui.slideshow.is_some(), "⌘↩ starts the slideshow");
    assert!(h.app.ui.fullscreen, "the slideshow goes full screen");
    assert_eq!(h.app.ui.right, panel, "⌘↩ preserves the panel rather than invoking Done");
}

/// ⌥⇧⌘← / → / ↑ are Lightroom's copy After → Before, Before → After and swap.
#[test]
fn before_after_settings_aliases_run() {
    let mut h = demo();
    let id = h.app.session.active().expect("a demo photo");
    let set = |h: &mut Headless, v: f64| {
        let r = h.request("engine.execute", json!({"command": "develop.set", "params": {"values": {"light.exposure": v}}}), T);
        assert_eq!(r["ok"], true, "{r}");
    };
    let exposure = |h: &Headless| h.app.session.develop_of(id).unwrap().light.exposure;
    let before = |h: &Headless| h.app.session.before.get(&id).map(|b| b.light.exposure);

    set(&mut h, 0.7);
    press(&mut h, "Left", &["cmd", "alt", "shift"]);
    assert_eq!(before(&h), Some(0.7), "⌥⇧⌘← copies After's settings to Before");
    assert_eq!(exposure(&h), 0.7, "the photo itself is untouched");

    set(&mut h, -0.5);
    let undo = h.app.session.undo.len();
    press(&mut h, "Right", &["cmd", "alt", "shift"]);
    assert_eq!(exposure(&h), 0.7, "⌥⇧⌘→ copies Before's settings to After");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");

    set(&mut h, -1.0);
    press(&mut h, "Up", &["cmd", "alt", "shift"]);
    assert_eq!(exposure(&h), 0.7, "⌥⇧⌘↑ swaps the two sides");
    assert_eq!(before(&h), Some(-1.0), "the old After became the new Before");
}

/// Home / End make the first / last photo of the view the active one — and yield to a key the user
/// gave to another command.
#[test]
fn home_and_end_select_the_ends_of_the_grid() {
    let mut h = demo();
    view(&mut h, json!({ "view": "photoGrid" }));
    let vis = h.app.session.visible_cloned();
    assert!(vis.len() > 2, "{vis:?}");
    let (first, last, middle) = (vis[0], *vis.last().unwrap(), vis[vis.len() / 2]);
    let r = h.request("engine.execute", json!({"command": "library.select", "params": {"ids": [middle.0], "active": middle.0}}), T);
    assert_eq!(r["ok"], true, "{r}");

    press(&mut h, "Home", &[]);
    assert_eq!(h.app.session.active(), Some(first), "Home goes to the first photo");
    press(&mut h, "End", &[]);
    assert_eq!(h.app.session.active(), Some(last), "End goes to the last photo");

    view(&mut h, json!({ "view": "detail" }));
    press(&mut h, "Home", &[]);
    assert_eq!(h.app.session.active(), Some(last), "Home is a grid key");

    view(&mut h, json!({ "view": "photoGrid" }));
    let r = h.app.run("app.setShortcut", json!({"id": "view.survey", "shortcut": "Home"})).unwrap();
    assert_eq!(r["shortcut"], "Home", "{r}");
    press(&mut h, "Home", &[]);
    assert_eq!(h.app.ui.view, crate::state::ViewMode::Survey, "the user's Home binding fires");
    assert_eq!(h.app.session.active(), Some(last), "the grid key stayed out of the way");
}

/// + and − nudge the slider under the pointer, one undo step each, like ↑ / ↓.
#[test]
fn plus_and_minus_nudge_the_slider() {
    let mut h = demo();
    view(&mut h, json!({ "view": "detail", "right": "edit", "openSections": ["light"], "histogram": false }));
    h.settle(SETTLE);
    let id = h.app.session.active().expect("a demo photo");
    let exposure = |h: &Headless| h.app.session.develop_of(id).unwrap().light.exposure;
    assert!(exposure(&h).abs() < 1e-9, "the demo photo starts at 0 EV");
    let r = h.request("ui.hoverWidget", json!({"id": "slider:light.exposure"}), T);
    assert_eq!(r["ok"], true, "{r}");

    let undo = h.app.session.undo.len();
    press(&mut h, "=", &[]);
    assert!((exposure(&h) - 0.05).abs() < 1e-9, "{}", exposure(&h));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    press(&mut h, "-", &[]);
    assert!(exposure(&h).abs() < 1e-9, "{}", exposure(&h));
    for (key, modifiers) in [("=", &["shift"][..]), ("+", &[][..])] {
        let undo = h.app.session.undo.len();
        press(&mut h, key, modifiers);
        assert!((exposure(&h) - 0.05).abs() < 1e-9, "{key}: {}", exposure(&h));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for {key}");
        press(&mut h, "-", &[]);
        assert!(exposure(&h).abs() < 1e-9, "{}", exposure(&h));
    }
    press(&mut h, "=", &["cmd"]);
    assert!(exposure(&h).abs() < 1e-9, "⌘= zooms, it does not nudge the slider");
}
