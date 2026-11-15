use serde_json::json;

use crate::Session;

fn demo() -> Session {
    Session::with_demo()
}

#[test]
fn auto_edits_every_selected_photo() {
    let mut s = demo();
    let vis = s.visible_cloned();
    s.execute("library.select", &json!({"ids": [vis[1].0, vis[2].0, vis[3].0]})).unwrap();

    let before: Vec<f64> = vis.iter().take(4).skip(1).map(|id| s.develop_of(*id).unwrap().light.exposure).collect();

    let undo_before = s.undo.len();
    let result = s.execute("develop.auto", &json!({})).unwrap();
    let undo_after = s.undo.len();

    assert_eq!(undo_after, undo_before + 1);

    // Every selected photo must have changed its exposure (per-photo analysis).
    for (i, id) in vis.iter().take(4).skip(1).enumerate() {
        let after = s.develop_of(*id).unwrap().light.exposure;
        assert_ne!(after, before[i], "photo {} exposure should change after auto", id.0);
    }

    // Return value: first target's tone as a JSON object.
    assert!(result.is_object());
}

#[test]
fn preset_applies_to_every_selected_photo() {
    let mut s = demo();
    let vis = s.visible_cloned();
    s.execute("library.select", &json!({"ids": [vis[1].0, vis[2].0, vis[3].0]})).unwrap();

    s.execute("preset.apply", &json!({"id": "lc.bw-high-contrast", "amount": 100})).unwrap();

    for id in vis.iter().take(4).skip(1) {
        assert_eq!(s.develop_of(*id).unwrap().treatment, lightcraft_develop::Treatment::Bw, "photo {} should be B&W after preset apply", id.0);
    }
}
