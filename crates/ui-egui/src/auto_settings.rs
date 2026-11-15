//! Auto Settings decodes a selection off the UI thread and commits it as one undo step.
use crate::LightcraftApp;
use lightcraft_catalog::{Op, PhotoId};
use lightcraft_develop::DevelopSettings;
use lightcraft_engine::media::{SourceLevel, SourceRef};
use lightcraft_pipeline::SourceInfo;
use serde_json::{Value, json};
use std::sync::{Arc, atomic::AtomicBool};

const LABEL: &str = "Auto Settings";
struct Input {
    id: PhotoId,
    settings: Arc<DevelopSettings>,
    source: SourceRef,
    info: SourceInfo,
}
struct Prepared {
    library: u64,
    commits: u64,
    inputs: Vec<Input>,
    progress_done: u64,
    progress_total: u64,
    partial: bool,
}
struct Completed {
    library: u64,
    commits: u64,
    /// Each photo's settings as they were read, and with Auto.
    settings: Vec<(PhotoId, Arc<DevelopSettings>, DevelopSettings)>,
    /// Photos left out because they could not be read.
    skipped: usize,
    failed: Option<String>,
}

fn prepare(app: &mut LightcraftApp, params: &Value) -> Result<Prepared, String> {
    let ids = app.session.targets(params).map_err(|e| e.to_string())?;
    let mut inputs = Vec::with_capacity(ids.len());
    for id in ids {
        let photo = app.session.catalog.photo(id).ok_or("Auto Settings: no such photo")?.clone();
        let settings = app.session.develop_of(id).ok_or("Auto Settings: no develop settings")?;
        let info = app.session.source_info(id);
        let source = app.session.media.source_ref(&photo, SourceLevel::Thumb);
        inputs.push(Input { id, settings, source, info });
    }
    let total = inputs.len() as u64;
    Ok(Prepared { library: app.session.library_generation(), commits: app.session.commits(), inputs, progress_done: 0, progress_total: total, partial: false })
}
impl Prepared {
    fn run(self, progress: Option<&lightcraft_engine::activity::TaskHandle>) -> Result<Completed, String> {
        let total = if self.progress_total > 0 { self.progress_total } else { self.inputs.len() as u64 };
        let done_before = self.progress_done;
        let partial = self.partial;
        let mut settings = Vec::with_capacity(self.inputs.len());
        let mut failed: Option<String> = None;
        let mut skipped = 0;
        for (i, input) in self.inputs.into_iter().enumerate() {
            if progress.is_some_and(|p| p.is_cancelled()) {
                return Err("Auto Settings cancelled".into());
            }
            // a photo that can't be read (offline, no decoder) is left out, not the whole selection
            let source = match input.source.load_source() {
                Ok(source) => source,
                Err(error) => {
                    failed.get_or_insert(error);
                    skipped += 1;
                    continue;
                }
            };
            let info = source.info_or(input.info);
            let auto = lightcraft_pipeline::auto::auto_tone(&source.image, &info, &input.settings);
            let mut develop = (*input.settings).clone();
            develop.light.exposure = auto.exposure;
            develop.light.contrast = auto.contrast;
            develop.light.highlights = auto.highlights;
            develop.light.shadows = auto.shadows;
            develop.light.whites = auto.whites;
            develop.light.blacks = auto.blacks;
            develop.color.vibrance = auto.vibrance;
            develop.color.saturation = auto.saturation;
            settings.push((input.id, input.settings, develop));
            if let Some(p) = progress {
                p.progress(done_before + i as u64 + 1, total);
            }
        }
        if progress.is_some_and(|p| p.is_cancelled()) {
            return Err("Auto Settings cancelled".into());
        }
        if let (true, Some(error)) = (settings.is_empty(), failed.clone()) {
            if !partial {
                return Err(error);
            }
        }
        Ok(Completed { library: self.library, commits: self.commits, settings, skipped, failed })
    }
}
fn apply(app: &mut LightcraftApp, completed: Completed) -> Result<usize, String> {
    // An undo or a live edit changes a photo's settings without a commit: compare the settings too.
    let moved = completed.settings.iter().any(|(id, before, _)| app.session.develop_of(*id).is_none_or(|now| now != *before));
    if app.session.library_generation() != completed.library || app.session.commits() != completed.commits || moved {
        return Err("Auto Settings was not applied because the library changed; try again".into());
    }
    let mut ops: Vec<Op> = Vec::new();
    let mut silent: Vec<(PhotoId, DevelopSettings)> = Vec::new();
    for (id, _, d) in completed.settings {
        if app.session.interaction.as_ref().is_some_and(|i| i.photo == id) {
            silent.push((id, d));
        } else if let Some(op) = app.session.develop_op(id, d, "Auto") {
            ops.push(op);
        }
    }
    if !ops.is_empty() || !silent.is_empty() {
        // through the command wrapper, so sidecars and the durable save follow as for any edit
        app.session
            .execute_fn("develop.auto", |s| {
                for (id, d) in silent {
                    s.set_develop(id, d, "Auto")?;
                }
                if !ops.is_empty() {
                    s.commit("Auto", Op::Batch { ops })?;
                }
                Ok(Value::Null)
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(completed.skipped)
}
#[cfg(target_arch = "wasm32")]
struct Acc {
    library: u64,
    commits: u64,
    settings: Vec<(PhotoId, Arc<DevelopSettings>, DevelopSettings)>,
    skipped: usize,
    failed: Option<String>,
}

#[cfg(target_arch = "wasm32")]
fn spawn_chunk(app: &mut LightcraftApp, mut chunks: std::collections::VecDeque<Prepared>, mut acc: Acc, cancel: Arc<AtomicBool>) -> Result<(), String> {
    let Some(chunk) = chunks.pop_front() else { return Ok(()) };
    let next_cancel = cancel.clone();
    crate::tasks::spawn_tracked(
        app,
        LABEL,
        "auto",
        cancel,
        move |progress| chunk.run(Some(&progress)),
        move |app, ctx, result| match result {
            Ok(done) => {
                acc.skipped += done.skipped;
                if acc.failed.is_none() {
                    acc.failed = done.failed.clone();
                }
                acc.settings.extend(done.settings);
                if chunks.is_empty() {
                    if acc.settings.is_empty() {
                        if let Some(error) = acc.failed {
                            app.toast(ctx, error);
                        } else {
                            app.toast(ctx, "Auto Settings cancelled".to_string());
                        }
                        return;
                    }
                    let completed = Completed { library: acc.library, commits: acc.commits, settings: acc.settings, skipped: acc.skipped, failed: acc.failed };
                    match apply(app, completed) {
                        Ok(0) => app.toast(ctx, crate::i18n::tr("Auto Settings").to_owned()),
                        Ok(skipped) => app.toast(ctx, format!("{}: {skipped} photo(s) could not be read", crate::i18n::tr("Auto Settings"))),
                        Err(error) => app.toast(ctx, error),
                    }
                } else if next_cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    app.toast(ctx, "Auto Settings cancelled".to_string());
                } else {
                    let _ = spawn_chunk(app, chunks, acc, next_cancel);
                }
            }
            Err(error) => app.toast(ctx, error),
        },
    )?;
    Ok(())
}

pub(crate) fn command(app: &mut LightcraftApp, params: &Value) -> Result<Value, String> {
    if app.tasks.is_running(LABEL) {
        return Err("Auto Settings is already running".into());
    }
    let prepared = prepare(app, params)?;
    let count = prepared.inputs.len();
    #[cfg(target_arch = "wasm32")]
    if count > 0 {
        let total = count as u64;
        let library = prepared.library;
        let commits = prepared.commits;
        let mut chunks = std::collections::VecDeque::new();
        for (i, input) in prepared.inputs.into_iter().enumerate() {
            chunks.push_back(Prepared { library, commits, progress_done: i as u64, progress_total: total, partial: true, inputs: vec![input] });
        }
        let acc = Acc { library, commits, settings: Vec::new(), skipped: 0, failed: None };
        let cancel = Arc::new(AtomicBool::new(false));
        spawn_chunk(app, chunks, acc, cancel)?;
        return Ok(json!({"pending": true, "photos": count}));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    crate::tasks::spawn_tracked(
        app,
        LABEL,
        "auto",
        cancel,
        move |progress| prepared.run(Some(&progress)),
        |app, ctx, result| match result.and_then(|done| apply(app, done)) {
            Ok(0) => app.toast(ctx, crate::i18n::tr("Auto Settings").to_owned()),
            Ok(skipped) => app.toast(ctx, format!("{}: {skipped} photo(s) could not be read", crate::i18n::tr("Auto Settings"))),
            Err(error) => app.toast(ctx, error),
        },
    )?;
    Ok(json!({"pending": true, "photos": count}))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn app() -> LightcraftApp {
        LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default())
    }
    #[test]
    fn selection_auto_is_deferred_and_commits_once() {
        let mut app = app();
        let ids: Vec<_> = app.session.catalog.photos().take(3).map(|p| p.id).collect();
        app.session.execute("library.select", &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()})).unwrap();
        let before = app.session.commits();
        let reply = app.run("develop.auto", json!({})).unwrap();
        assert_eq!(reply["pending"], true);
        assert_eq!(reply["photos"], 3);
        assert_eq!(app.session.commits(), before, "decoding cannot mutate the UI session");
        assert!(app.tasks.is_running(LABEL));
        assert!(crate::tasks::wait_for(&mut app, &egui::Context::default(), LABEL, std::time::Duration::from_secs(60)));
        assert_eq!(app.session.commits(), before + 1);
        for id in ids {
            assert_eq!(app.session.develop_of(id).unwrap().color.vibrance, 30.0);
        }
    }
    #[test]
    fn selection_auto_does_not_overwrite_newer_edits_or_another_library() {
        let mut app = app();
        let done = prepare(&mut app, &json!({})).unwrap().run(None).unwrap();
        app.session.execute("develop.set", &json!({"control":"light.exposure", "value": 1.25})).unwrap();
        let before = app.session.commits();
        assert!(apply(&mut app, done).is_err());
        assert_eq!(app.session.commits(), before);
        let done = prepare(&mut app, &json!({})).unwrap().run(None).unwrap();
        app.session = lightcraft_engine::Session::with_demo();
        assert!(apply(&mut app, done).is_err());
    }
    #[test]
    fn develop_auto_with_named_ids_and_no_selection_applies() {
        let mut app = app();
        let ids: Vec<_> = app.session.catalog.photos().take(2).map(|p| p.id).collect();
        app.session.execute("library.select", &json!({"ids": []})).unwrap();
        assert!(app.session.selection.ids.is_empty() && app.session.selection.active.is_none(), "no selection");
        let reply = app.session.execute("develop.auto", &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}));
        assert!(reply.is_ok(), "ids-only develop.auto must not require a selection: {reply:?}");
        for id in ids {
            assert_eq!(app.session.develop_of(id).unwrap().color.vibrance, 30.0);
        }
    }
}
