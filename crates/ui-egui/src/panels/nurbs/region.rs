//! Construction and live parameter editing for linked NURBS regions.
use super::{pstate, set_pstate};
use crate::VectorcraftApp;
use egui::Ui;
use serde_json::json;
use vectorcraft_doc::{LiveShape, NodeId, NodeKind};
const STATE: &str = "nurbs-region-editor";
#[derive(Clone, Default)]
struct State {
    uid: u64,
    id: Option<NodeId>,
    point: usize,
    interacting: bool,
    error: String,
}
fn finish(app: &mut VectorcraftApp, state: &mut State, cancel: bool) {
    if !state.interacting {
        return;
    }
    let result = if cancel { app.session.cancel_interaction() } else { app.session.commit_interaction() };
    state.interacting = false;
    if let Err(e) = result {
        state.error = e.to_string();
    }
    app.sync_views();
}
pub(super) fn settle(app: &mut VectorcraftApp, ctx: &egui::Context) -> bool {
    let mut state: State = pstate(ctx, STATE);
    if !state.interacting {
        return false;
    }
    if app.session.active().map(|s| s.uid) != Some(state.uid) {
        state.interacting = false;
        set_pstate(ctx, STATE, state);
        return false;
    }
    let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let selected = app.session.active().and_then(|s| (s.selection.objects.len() == 1).then(|| s.selection.objects.first().copied()).flatten());
    if cancel || selected != state.id || (!ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input()) {
        finish(app, &mut state, cancel);
        set_pstate(ctx, STATE, state);
        if cancel {
            ctx.input_mut(|i| i.consume_key(i.modifiers, egui::Key::Escape));
        }
    }
    cancel
}
/// Returns true when the selected object is a region, owning the panel body.
pub(super) fn show(app: &mut VectorcraftApp, ui: &mut Ui) -> bool {
    let can_create = app.session.active().is_some_and(|s| {
        s.selection.objects.len() == 2 && s.selection.objects.iter().all(|id| vectorcraft_doc::nurbs_region::source(&s.doc, *id).is_some())
    });
    if can_create && ui.button("Create NURBS Region").on_hover_text("Select two periodic NURBS curves. Larger bounds becomes outer; swap roles below. Sources remain editable; their own paint is cleared so it does not show through the new fill.").clicked() {
        let _=app.run("nurbs.region.create",json!({})); // app.run presents command errors.
    }
    let Some((id, mut spec, uid, problem, choices)) = app.session.active().and_then(|s| {
        if s.selection.objects.len() != 1 {
            return None;
        }
        let id = *s.selection.objects.first()?;
        let r = s.doc.node(id)?.extras().nurbs_region.as_deref()?.clone();
        let mut choices = vec![];
        s.doc.walk(|n| {
            if matches!(&n.kind,NodeKind::Path{live:Some(l),..} if matches!(&**l,LiveShape::Nurbs{..})) {
                let open = if vectorcraft_doc::nurbs_region::source(&s.doc, n.id).is_some() { "" } else { " (not periodic)" };
                choices.push((n.id, format!("{} #{}{open}", n.display_name(), n.id.0)));
            }
        });
        let problem = r.problem(&s.doc);
        Some((id, r, s.uid, problem, choices))
    }) else {
        let linked = app.session.active().and_then(|s| {
            let source = s.selection.objects.first().copied()?;
            let mut region = None;
            s.doc.walk(|n| {
                if n.extras().nurbs_region.as_deref().is_some_and(|r| r.outer == Some(source) || r.inner == Some(source)) {
                    region = Some(n.id);
                }
            });
            region
        });
        if let Some(id) = linked
            && ui.button("Select linked NURBS region").clicked()
            && app.session.select(|_, sel| sel.set([id])).is_ok()
        {
            app.sync_views();
        }
        return false;
    };
    ui.separator();
    ui.heading("NURBS Region");
    if let Some(e) = problem {
        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}. The object draws empty."));
    }
    let mut state: State = pstate(ui.ctx(), STATE);
    if state.uid != uid || state.id != Some(id) {
        finish(app, &mut state, false);
        state = State { uid, id: Some(id), ..Default::default() };
    }
    let original = spec.clone();
    let mut edit = None;
    for (title, slot) in [("Outer", &mut spec.outer), ("Inner", &mut spec.inner)] {
        ui.horizontal(|ui| {
            let text = slot
                .map(|id| choices.iter().find(|c| c.0 == id).map(|c| c.1.clone()).unwrap_or_else(|| format!("Missing #{}", id.0)))
                .unwrap_or_else(|| "Not assigned".into());
            egui::ComboBox::from_id_salt((STATE, title)).selected_text(text).width(175.0).show_ui(ui, |ui| {
                ui.selectable_value(slot, None, "Not assigned");
                for (id, label) in &choices {
                    ui.selectable_value(slot, Some(*id), label);
                }
            });
            ui.label(title);
            if ui.add_enabled(slot.is_some_and(|id| choices.iter().any(|c| c.0 == id)), egui::Button::new("Edit")).clicked() {
                edit = *slot;
            }
        });
    }
    if ui.button("Swap outer / inner").clicked() {
        std::mem::swap(&mut spec.outer, &mut spec.inner);
    }
    if spec.outer != original.outer || spec.inner != original.inner {
        finish(app, &mut state, false);
        if let Err(e) = app.run("nurbs.region.update", json!({"id":id.0,"outer":spec.outer,"inner":spec.inner})) {
            state.error = e.to_string();
        }
    }
    if let Some(source) = edit {
        finish(app, &mut state, false);
        if app.session.select(|_, sel| sel.set([source])).is_ok() {
            app.select_tool("nurbs");
            app.sync_views();
        }
    }
    ui.separator();
    egui::CollapsingHeader::new("Alpha transition spline").default_open(true).show(ui, |ui| {
        super::color::graph(ui, "Distance ratio → alpha", &mut spec.profile, 0.0, 1.0, &mut state.point);
        ui.horizontal(|ui| {
            if ui.button("Default").clicked() {
                spec.profile.points = vec![[0.0, 1.0], [1.0, 0.0]];
            }
            if ui.button("Linear").clicked() {
                spec.profile.points = vec![[0.0, 0.0], [1.0, 1.0]];
            }
            if ui.button("Invert").clicked() {
                for p in &mut spec.profile.points {
                    p[1] = 1.0 - p[1];
                }
            }
        });
        ui.small("x = distance to inner / sum of distances. Inside inner: full fill. Outside outer: empty.");
    });
    if spec.profile != original.profile && edit.is_none() {
        let begin = if state.interacting { Ok(()) } else { app.session.begin_interaction("Edit NURBS Region Alpha") };
        match begin {
            Ok(()) => {
                state.interacting = true;
                match app.session.preview("nurbs.region.update", &json!({"id":id.0,"profile":spec.profile})) {
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
    ui.small("Use standard Fill controls for the paint. Stroke is ignored. Geometry follows the source curves; edit or move those curves to reshape the region.");
    if !state.error.is_empty() {
        ui.colored_label(ui.visuals().error_fg_color, &state.error);
    }
    set_pstate(ui.ctx(), STATE, state);
    true
}

#[cfg(test)]
mod nurbs_region_tests {
    use super::*;
    use vectorcraft_engine::Session;
    fn frame(app: &mut VectorcraftApp) -> Vec<String> {
        fn texts(shape: &egui::Shape, output: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => output.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => {
                    for s in v {
                        texts(s, output);
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
        let mut result = vec![];
        for clipped in output.shapes {
            texts(&clipped.shape, &mut result);
        }
        result
    }
    #[test]
    fn nurbs_region_panel_and_layers_explain_missing_periodic_source() {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width":150,"height":150})).unwrap();
        let mut ids = vec![];
        for (x, size) in [(10.0, 100.0), (40.0, 40.0)] {
            let id=app.session.execute("nurbs.create",&json!({"controls":[{"x":x,"y":x},{"x":x+size,"y":x},{"x":x+size,"y":x+size},{"x":x,"y":x+size}],"periodic":true,"corners":[0,1,2,3]})).unwrap()["id"].as_u64().unwrap();
            ids.push(id);
        }
        let id = app.session.execute("nurbs.region.create", &json!({"outer":ids[0],"inner":ids[1]})).unwrap()["id"].as_u64().unwrap();
        let labels = frame(&mut app).join("\n");
        assert!(labels.contains("NURBS Region") && labels.contains("Outer") && labels.contains("Inner"), "{labels}");
        app.session.execute("nurbs.periodic", &json!({"id":ids[1],"periodic":false})).unwrap();
        let labels = frame(&mut app).join("\n");
        assert!(labels.contains("⚠") && labels.contains("periodic") && labels.contains("empty"), "{labels}");
        let node = app.session.active().unwrap().doc.node(NodeId(id)).unwrap();
        let name = crate::panels::layers::shown_name(&app, node, crate::i18n::Lang::EN);
        assert!(name.starts_with("⚠"), "{name}");
    }
}
