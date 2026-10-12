//! Headless tests of the point-curve editor: dragging points, resetting, presets.

use std::time::Duration;

use serde_json::json;

use crate::headless::Headless;
use crate::{LightcraftApp, Services};

const T: Duration = Duration::from_secs(20);

/// Detail view with the Edit panel's Curve flyout open on `channel`.
fn curve_open(channel: &str) -> Headless {
    let services = Services { png: None, ..Default::default() };
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    let mut h = Headless::new(app, [1200.0, 1400.0], 1.0);
    let r = h.request("ui.set", json!({"view": "detail"}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.app.ui.right = crate::state::RightPanel::Edit;
    if !h.app.ui.section_open("light") {
        h.app.ui.toggle_section("light");
    }
    if !h.app.ui.flyout_open("curve") {
        h.app.ui.toggle_flyout("curve");
    }
    h.app.ui.curve_channel = channel.into();
    h.step();
    h.step();
    h
}

fn exec(h: &mut Headless, command: &str, params: serde_json::Value) -> serde_json::Value {
    let r = h.request("engine.execute", json!({"command": command, "params": params}), T);
    assert_eq!(r["ok"], true, "{command}: {r}");
    r["result"].clone()
}

fn develop(h: &Headless) -> lightcraft_develop::DevelopSettings {
    let id = h.app.session.active().expect("active photo");
    (*h.app.session.develop_of(id).unwrap_or_default()).clone()
}

fn pts(v: &[lightcraft_geom::Point]) -> Vec<(f64, f64)> {
    v.iter().map(|p| (p.x, p.y)).collect()
}

/// Side of the (square) curve graph in screen points.
fn curve_side(h: &mut Headless) -> f64 {
    let r = h.request("ui.widgets", json!({"filter": "curve"}), T);
    let w = r["result"].as_array().unwrap().iter().find(|w| w["id"] == "curve").cloned().unwrap_or_else(|| panic!("curve widget on screen: {r}"));
    w["rect"][2].as_f64().unwrap()
}

#[test]
fn dragging_a_point_moves_it_in_both_axes_with_one_undo_step() {
    let mut h = curve_open("master");
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]}));
    h.step();
    let side = curve_side(&mut h);
    let undo_before = h.app.session.undo.len();
    // grab the middle point and drag it up-left: x 0.5 → 0.4, y 0.5 → 0.7
    let r = h.request("ui.dragWidget", json!({"id": "curve", "fx": 0.5, "fy": 0.5, "dx": -0.1 * side, "dy": -0.2 * side, "steps": 12}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    let c = pts(&develop(&h).curve.master);
    assert_eq!(c.len(), 3, "dragging moves the point, it doesn't add one: {c:?}");
    assert!((c[1].0 - 0.4).abs() < 0.03, "x follows the pointer: {c:?}");
    assert!((c[1].1 - 0.7).abs() < 0.03, "y follows the pointer: {c:?}");
    assert_eq!(h.app.session.undo.len(), undo_before + 1, "one undo step per drag");
}

fn click_widget(h: &mut Headless, id: &str, count: u64) {
    let r = h.request("ui.clickWidget", json!({"id": id, "count": count}), T);
    assert_eq!(r["ok"], true, "{id}: {r}");
    h.step();
}

const S_CURVE: [[f64; 2]; 4] = [[0.0, 0.0], [0.25, 0.15], [0.75, 0.85], [1.0, 1.0]];

#[test]
fn double_clicking_a_channel_resets_only_that_channel() {
    let mut h = curve_open("master");
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": S_CURVE}));
    exec(&mut h, "develop.curve", json!({"channel": "green", "points": S_CURVE}));
    h.step();
    click_widget(&mut h, "curveChannel:green", 2);
    let c = develop(&h).curve;
    assert!(c.green.is_empty(), "green resets: {:?}", c.green);
    assert_eq!(c.master.len(), 4, "the other channels stay");
    assert_eq!(h.app.ui.curve_channel, "green");
}

#[test]
fn reset_button_resets_every_curve() {
    let mut h = curve_open("red");
    for ch in ["master", "red", "blue"] {
        exec(&mut h, "develop.curve", json!({"channel": ch, "points": S_CURVE}));
    }
    exec(&mut h, "develop.set", json!({"control": "curve.highlights", "value": -40}));
    h.step();
    click_widget(&mut h, "button:curveReset", 1);
    let c = develop(&h).curve;
    assert_eq!(c, lightcraft_develop::ToneCurve::default());
}

#[test]
fn point_curve_preset_dropdown_applies_and_saves() {
    let mut h = curve_open("master");
    click_widget(&mut h, "dropdown:curvePreset", 1);
    click_widget(&mut h, "curvePreset:Medium Contrast", 1);
    let c = develop(&h).curve;
    assert_eq!(c.master.len(), 5, "the preset's S-curve: {:?}", c.master);
    assert!(c.master[1].y < 0.25 && c.master[3].y > 0.75);
    // shape it further, then save it from the dropdown (name prompt)
    exec(&mut h, "develop.curve", json!({"channel": "blue", "points": [[0.0, 0.08], [1.0, 1.0]]}));
    h.step();
    click_widget(&mut h, "dropdown:curvePreset", 1);
    click_widget(&mut h, "curvePresetMenu:save", 1);
    let mut dlg = h.app.ui.dialog.take().expect("a name prompt opens");
    match &mut dlg {
        crate::state::Dialog::TextPrompt { command, value, .. } => {
            assert_eq!(command, "curve.savePreset");
            *value = "Cool Shadows".into();
        }
        _ => panic!("a name prompt opens"),
    }
    let r = crate::panels::dialogs::confirm_dialog(&mut h.app, &dlg);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(h.app.session.curve_presets.len(), 1);
    // Linear from the dropdown flattens every channel
    click_widget(&mut h, "dropdown:curvePreset", 1);
    click_widget(&mut h, "curvePreset:Linear", 1);
    let c = develop(&h).curve;
    assert!(c.master.is_empty() && c.blue.is_empty());
    click_widget(&mut h, "dropdown:curvePreset", 1);
    click_widget(&mut h, "curvePreset:Cool Shadows", 1);
    assert_eq!(develop(&h).curve.blue.len(), 2);
}

/// Absolute window position of the graph's curve coordinate `(x, y)`: x to the right, y upwards.
fn graph_xy(h: &mut Headless, x: f64, y: f64) -> (f64, f64) {
    let r = h.request("ui.widgets", json!({"filter": "curve"}), T);
    let w = r["result"].as_array().unwrap().iter().find(|w| w["id"] == "curve").cloned().unwrap_or_else(|| panic!("curve widget on screen: {r}"));
    let rect: Vec<f64> = w["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    (rect[0] + x * rect[2], rect[1] + (1.0 - y) * rect[3])
}

/// A real pointer gesture on the graph: press at curve `from`, move `steps` times, release at curve
/// `to` — either may lie outside 0..1, which is the pointer leaving the graph.
fn drag_graph(h: &mut Headless, from: (f64, f64), to: (f64, f64), steps: u64) {
    let (x, y) = graph_xy(h, from.0, from.1);
    let (to_x, to_y) = graph_xy(h, to.0, to.1);
    let r = h.request("ui.drag", json!({"x": x, "y": y, "toX": to_x, "toY": to_y, "steps": steps}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
}

/// A real pointer click (or double-click) on the graph at curve coordinate `(x, y)`.
fn click_graph(h: &mut Headless, x: f64, y: f64, count: u64) {
    let (px, py) = graph_xy(h, x, y);
    let r = h.request("ui.click", json!({"x": px, "y": py, "count": count}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
}

fn vec_pts(v: &[[f64; 2]]) -> Vec<(f64, f64)> {
    v.iter().map(|p| (p[0], p[1])).collect()
}

/// Let egui's triple-click window (twice the 300 ms double-click delay) pass before a new gesture.
fn wait_clicks(h: &mut Headless) {
    for _ in 0..40 {
        h.step();
    }
}

#[test]
fn a_click_on_empty_space_adds_a_point_and_a_drag_then_moves_it() {
    let mut h = curve_open("master");
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]}));
    h.step();
    // clicking empty space adds one point there, between the two it landed between
    click_graph(&mut h, 0.3, 0.7, 1);
    let c = pts(&develop(&h).curve.master);
    assert_eq!(c.len(), 4, "clicking empty space adds one point: {c:?}");
    assert!((c[1].0 - 0.3).abs() < 0.02 && (c[1].1 - 0.7).abs() < 0.02, "the new point is where it was clicked: {c:?}");
    // …and it is the point the next drag moves, in both axes
    let undo_before = h.app.session.undo.len();
    drag_graph(&mut h, (0.3, 0.7), (0.4, 0.9), 6);
    let c = pts(&develop(&h).curve.master);
    assert_eq!(c.len(), 4, "dragging moves a point, it doesn't add one: {c:?}");
    assert!((c[1].0 - 0.4).abs() < 0.03 && (c[1].1 - 0.9).abs() < 0.03, "both axes follow the pointer: {c:?}");
    assert_eq!((c[0], c[2], c[3]), ((0.0, 0.0), (0.5, 0.5), (1.0, 1.0)), "the other points stay: {c:?}");
    assert_eq!(h.app.session.undo.len(), undo_before + 1, "one undo step per drag");
}

#[test]
fn a_two_frame_drag_captures_the_final_position_and_undo_redo_restore_it() {
    let mut h = curve_open("master");
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]}));
    h.step();
    let before = pts(&develop(&h).curve.master);
    let undo_before = h.app.session.undo.len();
    drag_graph(&mut h, (0.5, 0.5), (0.35, 0.8), 2);
    let after = pts(&develop(&h).curve.master);
    assert!(
        (after[1].0 - 0.35).abs() < 0.03 && (after[1].1 - 0.8).abs() < 0.03,
        "a two-frame drag still ends where the pointer was released: {after:?}"
    );
    assert_eq!(h.app.session.undo.len(), undo_before + 1, "one undo step for the whole drag");
    exec(&mut h, "edit.undo", json!({}));
    assert_eq!(pts(&develop(&h).curve.master), before, "undo restores the whole drag");
    exec(&mut h, "edit.redo", json!({}));
    assert_eq!(pts(&develop(&h).curve.master), after, "redo puts it back");
}

#[test]
fn dragging_a_point_with_dense_neighbours_leaves_them_alone() {
    let mut h = curve_open("master");
    // 0.005 apart: closer than the 0.01 the editor keeps between points, as an imported curve can be
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": [[0.0, 0.0], [0.5, 0.5], [0.505, 0.55], [0.51, 0.6], [1.0, 1.0]]}));
    h.step();
    drag_graph(&mut h, (0.505, 0.55), (0.8, 0.8), 6);
    let c = pts(&develop(&h).curve.master);
    assert_eq!(c.len(), 5, "{c:?}");
    assert!(c.windows(2).all(|w| w[0].0 < w[1].0), "the stored x order stays valid: {c:?}");
    assert_eq!(c[1], (0.5, 0.5), "the neighbour on the left stays: {c:?}");
    assert_eq!(c[3], (0.51, 0.6), "the neighbour on the right stays: {c:?}");
    assert!((c[2].0 - 0.505).abs() < 1e-9, "there is no room between them, so the point keeps its x: {c:?}");
    assert!((c[2].1 - 0.8).abs() < 0.03, "y still follows the pointer: {c:?}");
}

#[test]
fn dragging_an_unsorted_imported_curve_writes_back_a_valid_order() {
    let mut h = curve_open("master");
    // as an imported curve can arrive: out of order, and with two points at the same x
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": [[0.5, 0.6], [0.5, 0.4], [0.25, 0.2], [1.0, 1.0], [0.0, 0.0]]}));
    h.step();
    drag_graph(&mut h, (0.5, 0.6), (0.55, 0.7), 4);
    let c = pts(&develop(&h).curve.master);
    assert!(c.windows(2).all(|w| w[0].0 < w[1].0), "sorted, with no duplicate x left: {c:?}");
    assert_eq!(c.len(), 4, "the duplicate in x is merged as the renderer merges it: {c:?}");
    assert_eq!(c[0], (0.0, 0.0), "the imported endpoint survives: {c:?}");
    assert_eq!(c[3], (1.0, 1.0), "…at both ends: {c:?}");
    assert_eq!(c[1], (0.25, 0.2), "an untouched point stays: {c:?}");
    assert!((c[2].0 - 0.55).abs() < 0.03 && (c[2].1 - 0.7).abs() < 0.03, "the dragged point moved: {c:?}");
}

#[test]
fn dragging_endpoints_clamps_to_the_graph_edges_and_keeps_the_shortened_curve() {
    let mut h = curve_open("master");
    // a diagonal that stops short of the ends: the spline flattens black and white, so it is a curve
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": [[0.25, 0.25], [0.75, 0.75]]}));
    h.step();
    // the first endpoint, dragged out of the graph on both axes, stops at the corner
    drag_graph(&mut h, (0.25, 0.25), (-0.3, -0.3), 4);
    let c = pts(&develop(&h).curve.master);
    assert_eq!(c.len(), 2, "a shortened curve keeps its two points: {c:?}");
    assert_eq!(c[0], (0.0, 0.0), "the endpoint clamps at the corner: {c:?}");
    assert_eq!(c[1], (0.75, 0.75), "the other endpoint stays: {c:?}");
    // the last one stops at (1, 1)
    drag_graph(&mut h, (0.75, 0.75), (1.1, 1.05), 4);
    let c = pts(&develop(&h).curve.master);
    assert_eq!(c, vec![(0.0, 0.0), (1.0, 1.0)], "{c:?}");
}

#[test]
fn a_drag_on_one_channel_leaves_the_others_alone() {
    let mut h = curve_open("red");
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": S_CURVE}));
    exec(&mut h, "develop.curve", json!({"channel": "red", "points": [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]}));
    exec(&mut h, "develop.curve", json!({"channel": "blue", "points": [[0.0, 0.1], [1.0, 0.9]]}));
    exec(&mut h, "develop.set", json!({"control": "curve.refineSaturation", "value": 40}));
    h.step();
    drag_graph(&mut h, (0.5, 0.5), (0.4, 0.7), 6);
    let c = develop(&h).curve;
    let red = pts(&c.red);
    assert_eq!(red.len(), 3, "{red:?}");
    assert!((red[1].0 - 0.4).abs() < 0.03 && (red[1].1 - 0.7).abs() < 0.03, "the red point moved: {red:?}");
    assert_eq!(pts(&c.master), vec_pts(&S_CURVE), "the master curve is untouched");
    assert_eq!(pts(&c.green), Vec::<(f64, f64)>::new(), "green stays empty");
    assert_eq!(pts(&c.blue), vec![(0.0, 0.1), (1.0, 0.9)], "blue is untouched");
    assert_eq!(c.refine_saturation, 40.0, "Refine Saturation stays");
}

#[test]
fn double_click_deletes_an_interior_point_and_spares_the_endpoints() {
    let mut h = curve_open("master");
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": S_CURVE}));
    h.step();
    // a plain click on an existing point adds nothing
    click_graph(&mut h, 0.25, 0.15, 1);
    assert_eq!(pts(&develop(&h).curve.master), vec_pts(&S_CURVE), "a click on a point adds no duplicate");
    wait_clicks(&mut h);
    // double-clicking the interior point removes it
    click_graph(&mut h, 0.25, 0.15, 2);
    let c = pts(&develop(&h).curve.master);
    assert_eq!(c, vec![(0.0, 0.0), (0.75, 0.85), (1.0, 1.0)], "the interior point is gone: {c:?}");
    wait_clicks(&mut h);
    // …but an endpoint is kept
    click_graph(&mut h, 0.01, 0.01, 2);
    assert_eq!(pts(&develop(&h).curve.master), c, "an endpoint can't be deleted: {c:?}");
}

#[test]
fn parametric_sliders_keep_their_regions_and_the_point_curve_stays_editable() {
    let mut h = curve_open("parametric");
    let undo_before = h.app.session.undo.len();
    let r = h.request("ui.dragWidget", json!({"id": "slider:curve.highlights", "fx": 0.5, "fy": 0.5, "dx": 40.0, "dy": 0.0, "steps": 6}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    let c = develop(&h).curve;
    assert!(c.highlights > 0.0, "the Highlights slider moves its own region: {c:?}");
    assert_eq!((c.shadows, c.darks, c.lights), (0.0, 0.0, 0.0), "the other regions stay: {c:?}");
    assert_eq!(h.app.session.undo.len(), undo_before + 1, "one undo step per slider drag");
    // the point curve is still mouse-editable, and the parametric curve survives the edit
    wait_clicks(&mut h);
    click_widget(&mut h, "curveChannel:master", 1);
    exec(&mut h, "develop.curve", json!({"channel": "master", "points": [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]}));
    h.step();
    drag_graph(&mut h, (0.5, 0.5), (0.45, 0.65), 4);
    let c = develop(&h).curve;
    let m = pts(&c.master);
    assert!((m[1].0 - 0.45).abs() < 0.03 && (m[1].1 - 0.65).abs() < 0.03, "the point moved: {m:?}");
    assert!(c.highlights > 0.0, "the parametric curve survives the point edit: {c:?}");
}

#[test]
fn dragging_a_point_past_its_neighbour_is_clamped() {
    let mut h = curve_open("red");
    exec(&mut h, "develop.curve", json!({"channel": "red", "points": [[0.0, 0.0], [0.3, 0.3], [0.6, 0.6], [1.0, 1.0]]}));
    h.step();
    let side = curve_side(&mut h);
    // drag the point at 0.3 far to the right (past 0.6)
    let r = h.request("ui.dragWidget", json!({"id": "curve", "fx": 0.3, "fy": 0.7, "dx": 0.5 * side, "dy": 0.0, "steps": 12}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    let c = pts(&develop(&h).curve.red);
    assert_eq!(c.len(), 4, "{c:?}");
    assert!(c[1].0 < c[2].0 && c[1].0 > 0.55, "clamped just left of its neighbour: {c:?}");
    assert_eq!(c[2], (0.6, 0.6), "the neighbour stays: {c:?}");
}
