//! NURBS debris: linked sources, phase management and atomic live parameter edits.
use super::{pstate, set_pstate};
use crate::VectorcraftApp;
use egui::Ui;
use serde_json::json;
use vectorcraft_doc::nurbs_debris::DisplayMode;
use vectorcraft_doc::{NodeId, NodeKind};
const STATE: &str = "nurbs-debris-panel";
#[derive(Clone, Default)]
struct State {
    uid: u64,
    id: Option<NodeId>,
    interacting: bool,
    error: String,
    base: Option<NodeId>,
    source: Option<NodeId>,
    key: f64,
    selected_key: f64,
}
fn finish(app: &mut VectorcraftApp, s: &mut State, cancel: bool) {
    if !s.interacting {
        return;
    }
    let r = if cancel { app.session.cancel_interaction() } else { app.session.commit_interaction() };
    s.interacting = false;
    if let Err(e) = r {
        s.error = e.to_string();
    }
    app.sync_views();
}
pub(super) fn settle(app: &mut VectorcraftApp, ctx: &egui::Context) -> bool {
    let mut s: State = pstate(ctx, STATE);
    if !s.interacting {
        return false;
    }
    if app.session.active().map(|st| st.uid) != Some(s.uid) {
        s.interacting = false;
        set_pstate(ctx, STATE, s);
        return false;
    }
    let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let changed = app.session.active().is_none_or(|st| st.selection.objects.len() != 1 || st.selection.objects.first().copied() != s.id);
    if cancel || changed || (!ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input()) {
        finish(app, &mut s, cancel);
        set_pstate(ctx, STATE, s);
        if cancel {
            ctx.input_mut(|i| i.consume_key(i.modifiers, egui::Key::Escape));
        }
    }
    cancel
}
fn select(app: &mut VectorcraftApp, id: NodeId) {
    if app.session.select(|_, sel| sel.set([id])).is_ok() {
        app.sync_views();
    }
}
fn command(app: &mut VectorcraftApp, s: &mut State, id: &str, p: serde_json::Value) {
    finish(app, s, false);
    match app.run(id, p) {
        Ok(_) => s.error.clear(),
        Err(e) => s.error = e.to_string(),
    }
}
fn source_edit(app: &mut VectorcraftApp, s: &mut State, id: NodeId, key: Option<f64>) {
    finish(app, s, false);
    let p = if let Some(key) = key { json!({"id":id.0,"phase":key}) } else { json!({"id":id.0}) };
    match app.run("nurbs.debris.source", p) {
        Ok(v) => {
            if let Some(tool) = v["tool"].as_str() {
                app.select_tool(tool);
            }
        }
        Err(e) => s.error = e.to_string(),
    }
}
fn display_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, display: &mut DisplayMode) {
    egui::ComboBox::from_id_salt(id).selected_text(display.label()).width(90.0).show_ui(ui, |ui| {
        for mode in [DisplayMode::Normal, DisplayMode::Mute, DisplayMode::Outline] {
            ui.selectable_value(display, mode, mode.label());
        }
    });
}
pub(super) fn show(app: &mut VectorcraftApp, ui: &mut Ui) -> bool {
    let Some(st) = app.session.active() else {
        return false;
    };
    let uid = st.uid;
    let selection: Vec<_> = st.selection.objects.to_vec();
    let selected = if selection.len() == 1 { selection.first().copied() } else { None };
    let target = selected.and_then(|id| st.doc.node(id)?.extras().nurbs_debris.as_deref().cloned().map(|d| (id, d)));
    let mut curves = vec![];
    let mut objects = vec![];
    let mut attached = vec![];
    let mut linked = vec![];
    st.doc.walk(|n| {
        let independent = st
            .doc
            .ancestry(n.id)
            .is_none_or(|a| !a.into_iter().any(|id| id != n.id && st.doc.node(id).is_some_and(|n| n.extras().nurbs_debris.is_some())));
        if independent {
            let label = format!("{} #{}{}", n.display_name(), n.id.0, if n.visible { "" } else { " (hidden)" });
            if matches!(&n.kind,NodeKind::Path{live:Some(l),..} if matches!(&**l, vectorcraft_doc::LiveShape::Nurbs{..})) {
                curves.push((n.id, label.clone()));
            }
            if matches!(
                &n.kind,
                NodeKind::Path { .. }
                    | NodeKind::Group { .. }
                    | NodeKind::Compound { .. }
                    | NodeKind::Image(_)
                    | NodeKind::Mesh(_)
                    | NodeKind::Envelope { .. }
            ) {
                objects.push((n.id, label));
            }
        }
        if let Some(d) = n.extras().nurbs_debris.as_deref() {
            if d.base == selected && selected.is_some() {
                attached.push((n.id, n.display_name(), d.display));
            }
            for p in &d.phases {
                if Some(p.source) == selected {
                    linked.push((n.id, p.key, n.display_name()));
                }
            }
        }
    });
    let mut state: State = pstate(ui.ctx(), STATE);
    if state.uid != uid {
        state = State { uid, ..Default::default() };
    }
    if target.is_none() {
        let candidates: Vec<_> = curves.iter().filter(|c| selection.contains(&c.0)).cloned().collect();
        if selection.len() == 2 && !candidates.is_empty() {
            ui.collapsing("Attach debris from selection", |ui| {
                if !candidates.iter().any(|c| Some(c.0) == state.base) {
                    state.base = candidates.first().map(|c| c.0);
                }
                egui::ComboBox::from_id_salt("debris-create-base")
                    .selected_text(candidates.iter().find(|c| Some(c.0) == state.base).map(|c| c.1.as_str()).unwrap_or("Base NURBS"))
                    .show_ui(ui, |ui| {
                        for (id, label) in &candidates {
                            ui.selectable_value(&mut state.base, Some(*id), label);
                        }
                    });
                if ui.button("Attach NURBS Debris").clicked()
                    && let Some(base) = state.base
                    && let Some(source) = selection.iter().find(|id| **id != base)
                {
                    command(app, &mut state, "nurbs.debris.create", json!({"base":base.0,"source":source.0}));
                }
                ui.small("The original source is retained and hidden; use Edit source to change it.");
            });
        }
        if !attached.is_empty() {
            ui.collapsing("Attached debris", |ui| {
                for (id, name, mut display) in attached {
                    ui.horizontal(|ui| {
                        if ui.button(name).clicked() {
                            select(app, id);
                        }
                        let before = display;
                        display_picker(ui, ("debris-attached-display", id.0), &mut display);
                        if display != before {
                            command(app, &mut state, "nurbs.debris.update", json!({"id":id.0,"display":display}));
                        }
                    });
                }
            });
        }
        for (id, key, label) in linked {
            if ui.button(format!("Back to {label} (hide source)")).clicked() {
                command(app, &mut state, "nurbs.debris.source", json!({"id":id.0,"phase":key,"visible":false,"select":false}));
                select(app, id);
            }
        }
        if !state.error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &state.error);
        }
        set_pstate(ui.ctx(), STATE, state);
        return false;
    }
    let Some((id, spec)) = target else { return false };
    if state.id != Some(id) {
        finish(app, &mut state, false);
        state = State {
            uid,
            id: Some(id),
            key: spec.phases.last().map(|p| p.key + 1.0).unwrap_or(0.0),
            selected_key: spec.phases.first().map(|p| p.key).unwrap_or(0.0),
            ..Default::default()
        };
    }
    ui.heading("NURBS Debris");
    let mut display = spec.display;
    ui.horizontal(|ui| {
        ui.label("Display");
        display_picker(ui, "debris-display", &mut display);
    });
    if display != spec.display {
        command(app, &mut state, "nurbs.debris.update", json!({"id":id.0,"display":display}));
    }
    if let Some(e) = &spec.error {
        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}. This debris draws empty."));
    }
    let mut base = spec.base;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("debris-base")
            .selected_text(
                curves
                    .iter()
                    .find(|c| Some(c.0) == base)
                    .map(|c| c.1.clone())
                    .unwrap_or_else(|| base.map(|id| format!("Missing {}", id)).unwrap_or_else(|| "Not assigned".into())),
            )
            .width(170.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut base, None, "Not assigned");
                for (id, label) in &curves {
                    ui.selectable_value(&mut base, Some(*id), label);
                }
            });
        if ui.add_enabled(base.is_some(), egui::Button::new("Edit base")).clicked() {
            source_edit(app, &mut state, id, None);
        }
    });
    if base != spec.base {
        command(app, &mut state, "nurbs.debris.update", json!({"id":id.0,"base":base}));
    }
    let mut draft = spec.clone();
    ui.separator();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Parameter");
        let mut u = draft.parameter().unwrap_or(0.0);
        if ui.add(egui::DragValue::new(&mut u).speed(0.002).range(-10000.0..=10000.0)).changed() {
            match draft.set_parameter(u) {
                Ok(()) => changed = true,
                Err(e) => state.error = e,
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Scale");
        changed |= ui.add(egui::DragValue::new(&mut draft.scale).speed(0.01).range(-1000.0..=1000.0)).changed();
        ui.label("Offset");
        changed |= ui.add(egui::DragValue::new(&mut draft.offset).speed(0.5).range(-1e6..=1e6)).changed();
    });
    ui.horizontal(|ui| {
        ui.label("Blend");
        changed |= ui.add(egui::DragValue::new(&mut draft.blend).speed(0.01).range(-1e6..=1e6)).changed();
    });
    if let Ok(mut r) = draft.range() {
        ui.horizontal(|ui| {
            ui.label("u range");
            let a = ui.add(egui::DragValue::new(&mut r[0]).speed(0.002)).changed();
            let b = ui.add(egui::DragValue::new(&mut r[1]).speed(0.002)).changed();
            if a || b {
                match draft.set_range(r) {
                    Ok(()) => changed = true,
                    Err(e) => state.error = e,
                }
            }
        });
    }
    if changed {
        let start = if state.interacting { Ok(()) } else { app.session.begin_interaction("Edit NURBS Debris") };
        match start {
            Ok(()) => {
                state.interacting = true;
                match app.session.preview(
                    "nurbs.debris.update",
                    &json!({"id":id.0,"scale":draft.scale,"offset":draft.offset,"blend":draft.blend,"mapping":draft.map}),
                ) {
                    Ok(_) => {
                        state.error.clear();
                        app.sync_views();
                    }
                    Err(e) => {
                        state.error = e.to_string();
                        finish(app, &mut state, true);
                    }
                }
            }
            Err(e) => state.error = e.to_string(),
        }
    }
    egui::CollapsingHeader::new("Phases").default_open(true).show(ui, |ui| {
        for phase in &spec.phases {
            ui.push_id(phase.key.to_bits(), |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(state.selected_key == phase.key, format!("Phase {}", phase.key)).clicked() {
                        state.selected_key = phase.key;
                    }
                    if ui.button("Use").clicked() {
                        command(app, &mut state, "nurbs.debris.update", json!({"id":id.0,"blend":phase.key}));
                    }
                    if ui.button("Edit source").clicked() {
                        source_edit(app, &mut state, id, Some(phase.key));
                    }
                });
                ui.small(objects.iter().find(|o| o.0 == phase.source).map(|o| o.1.clone()).unwrap_or_else(|| format!("Missing {}", phase.source)));
            });
        }
        ui.separator();
        let available: Vec<_> = objects.iter().filter(|o| o.0 != id).collect();
        if !available.iter().any(|o| Some(o.0) == state.source) {
            state.source = available.first().map(|o| o.0);
        }
        egui::ComboBox::from_id_salt("debris-phase-source")
            .selected_text(available.iter().find(|o| Some(o.0) == state.source).map(|o| o.1.as_str()).unwrap_or("Phase source"))
            .width(220.0)
            .show_ui(ui, |ui| {
                for o in &available {
                    ui.selectable_value(&mut state.source, Some(o.0), &o.1);
                }
            });
        ui.horizontal(|ui| {
            ui.label("Phase key");
            ui.add(egui::DragValue::new(&mut state.key).speed(0.1).range(-1e6..=1e6));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(state.source.is_some(), egui::Button::new("Add / replace")).clicked()
                && let Some(source) = state.source
            {
                let key = state.key;
                command(app, &mut state, "nurbs.debris.phase.add", json!({"id":id.0,"source":source.0,"phase":key}));
            }
            if ui.button("Fix interpolated phase").clicked() {
                let key = state.key;
                command(app, &mut state, "nurbs.debris.phase.fix", json!({"id":id.0,"phase":key}));
            }
            if ui.button("Move key").clicked() {
                let (from, to) = (state.selected_key, state.key);
                command(app, &mut state, "nurbs.debris.phase.move", json!({"id":id.0,"phase":from,"newPhase":to}));
                state.selected_key = to;
            }
            if ui.button("Copy key").clicked() {
                let (from, to) = (state.selected_key, state.key);
                command(app, &mut state, "nurbs.debris.phase.move", json!({"id":id.0,"phase":from,"newPhase":to,"copy":true}));
            }
            if ui.add_enabled(spec.phases.len() > 1, egui::Button::new("Remove selected phase")).clicked() {
                let key = state.selected_key;
                command(app, &mut state, "nurbs.debris.phase.remove", json!({"id":id.0,"phase":key}));
            }
        });
        ui.small("Phase sources need equal vertex counts and compatible topology. Blend clamps outside the key range.");
    });
    ui.small("Values update live. One gesture = one Undo; Escape cancels. Images follow the curve through their four corners.");
    if !state.error.is_empty() {
        ui.colored_label(ui.visuals().error_fg_color, &state.error);
    }
    set_pstate(ui.ctx(), STATE, state);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;
    fn frame(app: &mut VectorcraftApp) -> String {
        fn collect(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, out);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(show(app, ui));
        });
        output.textures_delta.clear();
        let mut out = vec![];
        for c in output.shapes {
            collect(&c.shape, &mut out);
        }
        out.join("\n")
    }
    #[test]
    fn nurbs_debris_panel_shows_parameters_phases_and_repair_status() {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width":120,"height":120})).unwrap();
        let base = app.session.execute("nurbs.create", &json!({"controls":[{"x":0,"y":0},{"x":100,"y":0}]})).unwrap()["id"].as_u64().unwrap();
        let source = app.session.execute("nurbs.create", &json!({"controls":[{"x":20,"y":10},{"x":40,"y":20}]})).unwrap()["id"].as_u64().unwrap();
        let id = app.session.execute("nurbs.debris.create", &json!({"base":base,"source":source})).unwrap()["id"].as_u64().unwrap();
        let labels = frame(&mut app);
        for label in ["NURBS Debris", "Parameter", "Scale", "Offset", "Blend", "Phase 0", "Edit source", "Fix interpolated phase"] {
            assert!(labels.contains(label), "{label}: {labels}");
        }
        app.session.execute("nurbs.debris.update", &json!({"id":id,"base":null})).unwrap();
        let labels = frame(&mut app);
        assert!(labels.contains("not assigned") && labels.contains("empty"), "{labels}");
        let name = crate::panels::layers::shown_name(&app, app.session.active().unwrap().doc.node(NodeId(id)).unwrap(), crate::i18n::Lang::EN);
        assert!(name.starts_with('\u{26a0}'), "{name}");
    }
}
