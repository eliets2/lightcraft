//! `activity.list` / `activity.cancel` / `activity.save` / `activity.load` / `activity.validate` /
//! `activity.run`: the background tasks in flight and their batch queue
//! (see [`crate::activity`]).

use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, find_command, str_param};
use crate::Result;
use crate::activity::QueuedCommand;

const C: &str = "activity.queue";

/// The queue a fresh session starts with: the two read-only activity commands, so a batch
/// can be written, read back and re-run without anything to configure first.
fn default_queue() -> Vec<QueuedCommand> {
    vec![
        QueuedCommand { id: "activity.list".to_string(), params: Value::Object(Default::default()) },
        QueuedCommand { id: "activity.cancel".to_string(), params: json!({"all": true}) },
    ]
}

fn queued_is_runnable(id: &str) -> bool {
    id == "activity.list" || id == "activity.cancel"
}

fn validate_queue_entries(queue: &[QueuedCommand]) -> Result<()> {
    for q in queue {
        if q.id.is_empty() {
            return Err(bad(C, "queued command has no `id`"));
        }
        if q.id.starts_with("activity.") && !queued_is_runnable(&q.id) {
            return Err(bad(C, format!("`{}` may not be queued", q.id)));
        }
        if find_command(&q.id).is_none() {
            return Err(bad(C, format!("unknown command `{}`", q.id)));
        }
        if !q.params.is_object() {
            return Err(bad(C, format!("queued `{}` params must be an object", q.id)));
        }
    }
    Ok(())
}

/// The stored queue, with the default one filling an empty queue on first use.
fn ensure_queue(s: &crate::Session) -> Vec<QueuedCommand> {
    let queue = s.activity.queue();
    if queue.is_empty() {
        let fresh = default_queue();
        s.activity.set_queue(fresh.clone());
        fresh
    } else {
        queue
    }
}

fn queue_path(p: &Value) -> Result<String> {
    str_param(p, "path").map(|s| s.to_string()).ok_or_else(|| bad(C, "missing `path`"))
}

fn queue_file_write(path: &str, queue: &[QueuedCommand]) -> Result<()> {
    let text = serde_json::to_string_pretty(queue).map_err(|e| bad(C, e.to_string()))?;
    std::fs::write(path, text).map_err(|e| bad(C, format!("can't write {path}: {e}")))?;
    Ok(())
}

fn queue_file_read(path: &str) -> Result<Vec<QueuedCommand>> {
    let text = std::fs::read_to_string(path).map_err(|e| bad(C, format!("can't read {path}: {e}")))?;
    let queue: Vec<QueuedCommand> = serde_json::from_str(&text).map_err(|e| bad(C, format!("can't parse {path}: {e}")))?;
    validate_queue_entries(&queue)?;
    Ok(queue)
}

fn queue_run_one(s: &mut crate::Session, q: &QueuedCommand) -> Result<Value> {
    if q.id.is_empty() {
        return Err(bad(C, "queued command has no `id`"));
    }
    if q.id.starts_with("activity.") && !queued_is_runnable(&q.id) {
        return Err(bad(C, format!("`{}` may not be queued", q.id)));
    }
    if !q.params.is_object() {
        return Err(bad(C, format!("queued `{}` params must be an object", q.id)));
    }
    s.execute(&q.id, &q.params).map_err(|e| bad(C, format!("queued `{}` failed: {e}", q.id)))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "activity.list",
            "Background Tasks",
            [],
            None,
            "{} → {tasks: [{id, kind, label, done, total, unit: count|bytes|percent, detail, cancellable, cancelling, ageMs}]} — the long-running tasks in flight (imports, exports, preview builds, downloads, the face scan…), oldest first; total 0 = not known yet",
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
        cmd!(query "activity.validate", "Validate Batch Queue", [], None, "{} → {queue: [{id, params}], valid, errors: [..]} — check the queued command ids against the registry without running them", always, |s, _| {
            let queue = ensure_queue(s);
            let mut errors = Vec::new();
            for q in &queue {
                if q.id.is_empty() {
                    errors.push("queued command has no `id`".to_string());
                } else if q.id.starts_with("activity.") && !queued_is_runnable(&q.id) {
                    errors.push(format!("`{}` may not be queued", q.id));
                } else if find_command(&q.id).is_none() {
                    errors.push(format!("unknown command `{}`", q.id));
                } else if !q.params.is_object() {
                    errors.push(format!("queued `{}` params must be an object", q.id));
                }
            }
            let valid = errors.is_empty();
            Ok(json!({"queue": queue, "valid": valid, "errors": errors}))
        }),
        cmd!(
            "activity.save",
            "Save Batch Queue",
            [],
            None,
            "{path} → {queue: [{id, params}]} — write the queued commands to a JSON file (a fresh session starts with activity.list and activity.cancel queued)",
            always,
            |s, p| {
                let queue = ensure_queue(s);
                if let Some(path) = str_param(p, "path") {
                    queue_file_write(path, &queue)?;
                }
                Ok(json!({"queue": queue}))
            }
        ),
        cmd!(
            "activity.load",
            "Load Batch Queue",
            [],
            None,
            "{path} | {queue: [{id, params}]} → {queued: n} — read queued commands from a JSON file (or take them from params) so the queue can be re-run",
            always,
            |s, p| {
                let queue = if let Some(items) = p.get("queue").filter(|v| !v.is_null()) {
                    let parsed: Vec<QueuedCommand> = serde_json::from_value(items.clone()).map_err(|e| bad(C, format!("can't parse `queue`: {e}")))?;
                    validate_queue_entries(&parsed)?;
                    parsed
                } else if str_param(p, "path").is_some() {
                    queue_file_read(&queue_path(p)?)?
                } else {
                    ensure_queue(s)
                };
                let n = s.activity.set_queue(queue);
                Ok(json!({"queued": n}))
            }
        ),
        cmd!(
            "activity.run",
            "Run Batch Queue",
            [],
            None,
            "{} → {queued: n, ran: n, results: [..]} — run the queued commands in order; {id, params} appends one command and counts it as queued",
            always,
            |s, p| {
                if let Some(id) = p.get("id").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    let params = p.get("params").cloned().unwrap_or_else(|| Value::Object(Default::default()));
                    s.activity.enqueue(QueuedCommand { id: id.to_string(), params });
                    return Ok(json!({"queued": s.activity.queue().len()}));
                }
                let queue = ensure_queue(s);
                let mut results = Vec::new();
                let mut ran = 0usize;
                for q in &queue {
                    let r = queue_run_one(s, q);
                    let value = match r {
                        Ok(v) => v,
                        Err(e) => {
                            results.push(json!({"error": e.to_string()}));
                            break;
                        }
                    };
                    ran += 1;
                    results.push(value);
                }
                Ok(json!({"queued": queue.len(), "ran": ran, "results": results}))
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
    fn save_and_reload_queue_preserves_command_ids() {
        let mut s = Session::new();
        for id in ["activity.save", "activity.load", "activity.validate", "activity.run"] {
            assert!(s.execute(id, &json!({})).is_ok(), "missing queue command: {id}");
        }
        let saved = s.execute("activity.save", &json!({"path": "queue.json"})).unwrap();
        let ids: Vec<&str> = saved["queue"].as_array().unwrap().iter().map(|q| q["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["activity.list", "activity.cancel"]);
        let mut t = Session::new();
        t.execute("activity.load", &json!({"path": "queue.json"})).unwrap();
        assert_eq!(t.execute("activity.run", &json!({})).unwrap()["queued"], 2);
    }
}
