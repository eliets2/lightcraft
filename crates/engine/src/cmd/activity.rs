//! `activity.list` / `activity.cancel` / `activity.pause` / `activity.resume`: the background tasks in flight (see [`crate::activity`]).

use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "activity.list",
            "Background Tasks",
            [],
            None,
            "{} → {tasks: [{id, kind, label, done, total, unit: count|bytes|percent, detail, cancellable, cancelling, paused, ageMs}]} — the long-running tasks in flight (imports, exports, preview builds, downloads, the face scan…), oldest first; total 0 = not known yet",
            always,
            |s, _| Ok(json!({"tasks": s.activity.list()}))
        ),
        cmd!(
            "activity.cancel",
            "Cancel Background Task",
            [],
            None,
            "{id} | {all: true} → {cancelled: n} — ask a task (or every cancellable task) to stop; it stops at its next file or photo (a preview build also stops between a photo's own previews). An unknown id (it may have just finished) or a task that can't be cancelled is an error",
            always,
            |s, p| {
                const C: &str = "activity.cancel";
                if p.get("all").and_then(Value::as_bool) == Some(true) {
                    return Ok(json!({"cancelled": s.activity.cancel_all()}));
                }
                let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad(C, "id or all"))?;
                s.activity.cancel(id).map_err(|e| bad(C, e))?;
                Ok(json!({"cancelled": 1}))
            }
        ),
        cmd!(
            "activity.pause",
            "Pause Background Task",
            [],
            None,
            "{id} → {paused: 1} — ask a task to wait; it does not cancel it. An unknown id (it may have just finished) is an error",
            always,
            |s, p| {
                const C: &str = "activity.pause";
                let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad(C, "id"))?;
                s.activity.set_paused(id, true).map_err(|e| bad(C, e))?;
                Ok(json!({"paused": 1}))
            }
        ),
        cmd!(
            "activity.resume",
            "Resume Background Task",
            [],
            None,
            "{id} → {resumed: 1} — ask a paused task to continue. An unknown id (it may have just finished) is an error",
            always,
            |s, p| {
                const C: &str = "activity.resume";
                let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad(C, "id"))?;
                s.activity.set_paused(id, false).map_err(|e| bad(C, e))?;
                Ok(json!({"resumed": 1}))
            }
        ),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;
    use crate::activity::Cancel;

    #[test]
    fn list_and_cancel_through_execute() {
        let mut s = Session::new();
        let g = s.activity.start("export", "Exporting", Cancel::Yes);
        let r = s.execute("activity.list", &json!({})).unwrap();
        assert_eq!(r["tasks"][0]["kind"], "export");
        assert_eq!(r["tasks"][0]["cancellable"], true);
        let r = s.execute("activity.cancel", &json!({"id": g.id()})).unwrap();
        assert_eq!(r["cancelled"], 1);
        assert!(g.is_cancelled());
    }

    #[test]
    fn cancel_all_and_errors() {
        let mut s = Session::new();
        let (_a, _b) = (s.activity.start("export", "E", Cancel::Yes), s.activity.start("faces", "F", Cancel::No));
        assert_eq!(s.execute("activity.cancel", &json!({"all": true})).unwrap()["cancelled"], 1);
        assert!(s.execute("activity.cancel", &json!({"id": 999})).is_err());
        assert!(s.execute("activity.cancel", &json!({})).is_err());
    }

    #[test]
    fn paused_task_reports_paused_then_resumes() {
        let mut s = Session::new();
        let g = s.activity.start("export", "Exporting", Cancel::Yes);

        // a fresh task is not paused; pausing it must not cancel it, and the row must say so
        let r = s.execute("activity.list", &json!({})).unwrap();
        assert_eq!(r["tasks"][0]["paused"], false, "a fresh task is not paused: {:?}", r["tasks"][0]);

        let r = s.execute("activity.pause", &json!({"id": g.id()})).unwrap();
        assert_eq!(r["paused"], 1);
        assert!(!g.is_cancelled(), "pausing must not cancel the task");

        let r = s.execute("activity.list", &json!({})).unwrap();
        assert_eq!(r["tasks"][0]["paused"], true, "the paused row says so: {:?}", r["tasks"][0]);

        let r = s.execute("activity.resume", &json!({"id": g.id()})).unwrap();
        assert_eq!(r["resumed"], 1);

        let r = s.execute("activity.list", &json!({})).unwrap();
        assert_eq!(r["tasks"][0]["paused"], false, "the resumed row says so: {:?}", r["tasks"][0]);
    }
}
