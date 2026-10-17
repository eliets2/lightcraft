//! The first-run Welcome tour: Back / Next walk its pages, Skip and the last page's Get Started
//! close it. (The auto-open at launch is decided by `startup::open_at_startup`, unit-tested there:
//! a headless host never gains the modal, so the tour is placed by hand here.)

use crate::headless::Headless;
use crate::state::Dialog;
use crate::{LightcraftApp, Services};
use serde_json::json;
use std::time::Duration;

const T: Duration = Duration::from_secs(20);

/// The app with the tour open on `page` (the last page of [`crate::panels::dialogs::WELCOME_PAGES`]
/// is `WELCOME_PAGES.len() - 1`).
fn tour(page: usize) -> Headless {
    let services = Services { png: None, ..Default::default() };
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    app.ui.dialog = Some(Dialog::Welcome { page });
    let mut h = Headless::new(app, [1200.0, 800.0], 1.0);
    // fonts land on the second frame, and the dialog window settles its place a frame or two later;
    // clicks must aim at the settled layout
    h.step();
    h.settle(Duration::from_secs(5));
    h
}

fn page_of(h: &Headless) -> Option<usize> {
    match &h.app.ui.dialog {
        Some(Dialog::Welcome { page }) => Some(*page),
        _ => None,
    }
}

/// Click an automation id and let the injected press/release play out (they can straddle a frame).
fn click(h: &mut Headless, id: &str) {
    let r = h.request("ui.clickWidget", json!({"id": id}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.step();
}

#[test]
fn welcome_pages_walk_back_next_and_skip_closes() {
    let mut h = tour(0);
    // Back is disabled on the first page: the click lands on nothing and the tour stays.
    click(&mut h, "button:welcomeBack");
    assert_eq!(page_of(&h), Some(0), "{:?}", h.app.ui.dialog);
    click(&mut h, "button:welcomeNext");
    assert_eq!(page_of(&h), Some(1), "{:?}", h.app.ui.dialog);
    click(&mut h, "button:welcomeBack");
    assert_eq!(page_of(&h), Some(0), "{:?}", h.app.ui.dialog);
    click(&mut h, "button:welcomeSkip");
    assert!(h.app.ui.dialog.is_none(), "{:?}", h.app.ui.dialog);
}

#[test]
fn welcome_last_page_gets_started_closes() {
    let last = crate::panels::dialogs::WELCOME_PAGES.len() - 1;
    let mut h = tour(last);
    click(&mut h, "button:welcomeNext");
    assert!(h.app.ui.dialog.is_none(), "{:?}", h.app.ui.dialog);
}
