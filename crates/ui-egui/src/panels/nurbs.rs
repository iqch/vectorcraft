//! NURBS panel: group conversion, pasted hierarchy, source-range and scale editing.
use super::{pstate, set_pstate};
use crate::VectorcraftApp;
use egui::Ui;
use serde_json::json;
use vectorcraft_doc::{LiveShape, NodeId, NodeKind};
use vectorcraft_geom::nurbs::NurbsPath;

pub const ID: &str = "nurbs";
#[path = "nurbs/color.rs"]
mod color;
mod debris;
#[path = "nurbs/range.rs"]
mod range;
mod region;
#[path = "nurbs/settings.rs"]
mod settings;
#[derive(Clone, Default)]
struct State {
    doc: u64,
    bar: Option<range::Drag>,
    id: Option<NodeId>,
    path: Vec<String>,
    last: Option<NurbsPath>,
    start: f64,
    end: f64,
    scale: f64,
    length: bool,
    error: String,
}
fn selected(app: &VectorcraftApp) -> Option<(NodeId, String, NurbsPath, u64)> {
    let st = app.session.active()?;
    if st.selection.objects.len() != 1 {
        return None;
    }
    let id = *st.selection.objects.first()?;
    let node = st.doc.node(id)?;
    let NodeKind::Path { live: Some(live), .. } = &node.kind else { return None };
    let LiveShape::Nurbs { curve, .. } = &**live else { return None };
    Some((id, node.name.clone().unwrap_or_else(|| "NURBS".into()), curve.clone(), st.uid))
}
fn pick(app: &mut VectorcraftApp, id: NodeId, path: &[String]) {
    if app.session.tool_id() != "nurbs" {
        app.select_tool("nurbs");
    }
    app.session.set_tool_option("pastedDocument", &json!(app.session.active().map(|s| s.uid)));
    app.session.set_tool_option("pastedTarget", &json!(id.0));
    app.session.set_tool_option("pastedPath", &json!(path));
    app.session.set_tool_option("pastedMode", &json!(!path.is_empty()));
}
fn picked(app: &VectorcraftApp, id: NodeId) -> Vec<String> {
    if app.session.tool_id() != "nurbs" {
        return vec![];
    }
    let v = app.session.tool_options();
    if v["pastedDocument"].as_u64() != app.session.active().map(|s| s.uid) || v["pastedTarget"].as_u64() != Some(id.0) {
        return vec![];
    }
    v["pastedPath"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}
fn tree(
    ui: &mut Ui,
    c: &NurbsPath,
    path: &mut Vec<String>,
    selected: &[String],
    clicked: &mut Option<Vec<String>>,
    toggled: &mut Option<(Vec<String>, bool)>,
) {
    for paste in c.pastes() {
        path.push(paste.name.clone());
        ui.push_id(path.clone(), |ui| {
            ui.horizontal(|ui| {
                ui.add_space((path.len().saturating_sub(1) * 12) as f32);
                let mut enabled = paste.enabled;
                if ui.checkbox(&mut enabled, "").on_hover_text("Enable / disable paste").changed() {
                    *toggled = Some((path.clone(), enabled));
                }
                if ui.selectable_label(selected == path.as_slice(), &paste.name).on_hover_text("Select this pasted curve on the canvas").clicked() {
                    *clicked = Some(path.clone());
                }
            });
            tree(ui, &paste.curve, path, selected, clicked, toggled);
        });
        path.pop();
    }
}
fn refresh(state: &mut State, id: NodeId, root: &NurbsPath, path: &[String]) {
    state.id = Some(id);
    state.path = path.to_vec();
    state.last = Some(root.clone());
    state.error.clear();
    if let Some((name, parent_path)) = path.split_last() {
        if let Ok(parent) = root.materialize_at(parent_path) {
            if let Ok((u, l)) = parent.paste_ranges(name) {
                let mut range = if state.length { l } else { u };
                if !state.length && parent.is_periodic() && (range[0] < 0.0 || range[1] > 2.0) {
                    let span = range[1] - range[0];
                    range[0] = range[0].rem_euclid(1.0);
                    range[1] = range[0] + span;
                }
                state.start = range[0];
                state.end = range[1];
            }
            if let Some(p) = parent.pastes().iter().find(|p| &p.name == name) {
                state.scale = p.scale;
            }
        }
    }
}
fn changed_model(root: &NurbsPath, state: &State, remove: bool) -> Result<NurbsPath, String> {
    let (name, parent_path) = state.path.split_last().ok_or_else(|| "Select a pasted curve".to_string())?;
    let parent = root.materialize_at(parent_path).map_err(|e| e.to_string())?;
    let parent = if remove {
        parent.remove_paste(name)
    } else {
        let range = if state.length { parent.length_range_to_parameters([state.start, state.end]) } else { Ok([state.start, state.end]) }
            .map_err(|e| e.to_string())?;
        parent.update_paste(name, Some(range), Some(state.scale))
    }
    .map_err(|e| e.to_string())?;
    root.replace_source_at(parent_path, parent).map_err(|e| e.to_string())
}
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    if debris::show(app, ui) {
        return;
    }
    if region::show(app, ui) {
        return;
    }
    let group = app.session.active().and_then(|st| {
        if st.selection.objects.len() != 1 {
            return None;
        }
        let id = *st.selection.objects.first()?;
        let NodeKind::Group { children, clip: false } = &st.doc.node(id)?.kind else { return None };
        let count = children.iter().filter(|n| matches!(&n.kind,NodeKind::Path{live:Some(l),..} if matches!(&**l,LiveShape::Nurbs{..}))).count();
        (count >= 2).then_some(id)
    });
    if ui
        .add_enabled(group.is_some(), egui::Button::new("Paste NURBS from Group"))
        .on_hover_text("First NURBS in group order is the base; other object kinds are ignored")
        .clicked()
    {
        if let Some(group) = group {
            if let Ok(v) = app.run("nurbs.paste", json!({"id":group.0})) {
                if let Some(id) = v["id"].as_u64() {
                    let path =
                        v["pasted"].as_array().and_then(|a| a.first()).and_then(|v| v.as_str()).map(|s| vec![s.to_string()]).unwrap_or_default();
                    pick(app, NodeId(id), &path);
                }
            }
        }
    }
    ui.separator();
    let Some((id, label, root, doc)) = selected(app) else {
        ui.label("Select a NURBS curve, a NURBS group, or two periodic curves to create a region.");
        settings::show(app, ui, None);
        return;
    };
    let mut path = picked(app, id);
    if root.source_at(&path).is_none() {
        path.clear();
        pick(app, id, &path);
    }
    let mut toggled = None;
    let mut clicked = None;
    if ui.selectable_label(path.is_empty(), format!("{label} · Base")).clicked() {
        clicked = Some(vec![]);
    }
    egui::ScrollArea::vertical()
        .id_salt("nurbs-tree")
        .max_height(220.0)
        .show(ui, |ui| tree(ui, &root, &mut vec![], &path, &mut clicked, &mut toggled));
    if let Some(value) = clicked {
        pick(app, id, &value);
        path = value;
    }
    let mut row_error = None;
    if let Some((changed, enabled)) = toggled {
        if let Some((name, parent_path)) = changed.split_last() {
            let edited = root
                .materialize_at(parent_path)
                .and_then(|p| p.set_paste_enabled(name, enabled))
                .and_then(|p| root.replace_source_at(parent_path, p));
            match edited {
                Ok(c) => match app.run("nurbs.update", json!({"id":id.0,"curve":c,"closed":false})) {
                    Ok(_) => {
                        pick(app, id, &changed);
                        return;
                    }
                    Err(e) => row_error = Some(e.to_string()),
                },
                Err(e) => row_error = Some(e.to_string()),
            }
        }
    }
    let mut state: State = pstate(ui.ctx(), ID);
    if state.doc != doc {
        state = State::default();
        state.doc = doc;
    }
    if state.id != Some(id) || state.path != path || state.last.as_ref() != Some(&root) {
        refresh(&mut state, id, &root, &path);
    }
    if let Some(error) = row_error {
        app.ui.status = error.clone();
        state.error = error;
    }
    color::show(app, ui, id, &root, &path, doc);
    if path.is_empty() {
        ui.separator();
        if !root.pastes().is_empty() && ui.button("Select Pasted on Canvas").clicked() {
            if let Some(first) = root.pastes().first() {
                pick(app, id, &[first.name.clone()]);
            }
        }
        ui.label("Base: edit control vertices with the NURBS tool.");
        ui.small("Choose a pasted row to select and drag its curve along the base.");
        set_pstate(ui.ctx(), ID, state);
        return;
    }
    ui.separator();
    let Some((selected_name, parent_path)) = path.split_last() else { return };
    if let Ok(parent) = root.materialize_at(parent_path) {
        if let Some(p) = parent.pastes().iter().find(|p| &p.name == selected_name) {
            let mut enabled = p.enabled;
            if ui.checkbox(&mut enabled, "Enabled").on_hover_text("Keep this paste in the model but temporarily restore the base interval").changed()
            {
                match parent.set_paste_enabled(selected_name, enabled).and_then(|c| root.replace_source_at(parent_path, c)) {
                    Ok(c) => {
                        if let Err(e) = app.run("nurbs.update", json!({"id":id.0,"curve":c,"closed":false})) {
                            state.error = e.to_string();
                        }
                    }
                    Err(e) => state.error = e.to_string(),
                }
            }
        }
    }
    range::show(app, ui, id, &root, &path, &mut state);
    ui.label(path.last().map_or("Pasted NURBS", String::as_str));
    ui.horizontal(|ui| {
        ui.label("Range:");
        let before = state.length;
        ui.selectable_value(&mut state.length, false, "Parameter");
        ui.selectable_value(&mut state.length, true, "Length");
        if before != state.length {
            refresh(&mut state, id, &root, &path);
        }
    });
    for (label, value, speed) in [("Start", &mut state.start, 0.001), ("End", &mut state.end, 0.001), ("Scale", &mut state.scale, 0.01)] {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::DragValue::new(value).speed(speed).max_decimals(5));
        });
    }
    ui.small("Drag the selected curve along its base. Esc cancels; Delete removes it.");
    let mut duplicate = false;
    let mut apply = false;
    let mut remove = false;
    ui.horizontal_wrapped(|ui| {
        apply = ui.button("Apply").clicked();
        if ui.button("Reset").clicked() {
            refresh(&mut state, id, &root, &path);
        }
        duplicate = ui.button("Duplicate").clicked();
        remove = ui.button("Remove Paste").clicked();
    });
    if duplicate {
        if let Some((name, parent_path)) = path.split_last() {
            if let Ok(parent) = root.materialize_at(parent_path) {
                match parent.duplicate_paste(name, None) {
                    Ok((parent, name)) => match root.replace_source_at(parent_path, parent) {
                        Ok(c) => match app.run("nurbs.update", json!({"id":id.0,"curve":c,"closed":false})) {
                            Ok(_) => {
                                let mut next = parent_path.to_vec();
                                next.push(name);
                                pick(app, id, &next);
                                state.last = None;
                                app.ui.status = "Copy created disabled. Move it to a free interval, then enable it.".into();
                            }
                            Err(e) => state.error = e.to_string(),
                        },
                        Err(e) => state.error = e.to_string(),
                    },
                    Err(e) => state.error = e.to_string(),
                }
            }
        }
    }
    if apply || remove {
        match changed_model(&root, &state, remove) {
            Ok(curve) => match app.run("nurbs.update", json!({"id":id.0,"curve":curve,"closed":false})) {
                Ok(_) => {
                    state.error.clear();
                    state.last = None;
                    if remove {
                        let parent = &path[..path.len() - 1];
                        pick(app, id, parent);
                    }
                }
                Err(e) => state.error = e.to_string(),
            },
            Err(e) => state.error = e,
        }
    }
    if !state.error.is_empty() {
        ui.colored_label(egui::Color32::from_rgb(220, 100, 80), &state.error);
    }
    set_pstate(ui.ctx(), ID, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_geom::{
        Point,
        nurbs::{ControlPoint, PasteMode},
    };
    fn line(points: &[(f64, f64)]) -> NurbsPath {
        NurbsPath::from_controls(points.iter().map(|&(x, y)| ControlPoint::new(Point::new(x, y), 1.0)).collect()).unwrap()
    }
    #[test]
    fn nurbs_panel_edits_nested_length_binding_in_the_live_parent_frame() {
        let inner = line(&[(25.0, 0.0), (30.0, 8.0), (35.0, 0.0)]);
        let outer = line(&[(20.0, 0.0), (40.0, 0.0)]).paste_curve(inner, "Inner".into(), PasteMode::Length, 1.0).unwrap();
        let root = line(&[(0.0, 0.0), (100.0, 0.0)]).paste_curve(outer, "Outer".into(), PasteMode::Normalized, 1.0).unwrap();
        let root = root.set_control(1, ControlPoint::new(Point::new(200.0, 0.0), 1.0)).unwrap();
        let path = vec!["Outer".into(), "Inner".into()];
        let mut state = State::default();
        refresh(&mut state, NodeId(1), &root, &path);
        assert!((state.start - 0.125).abs() < 1e-6);
        state.start = 0.5;
        state.end = 0.75;
        state.scale = 2.0;
        let edited = changed_model(&root, &state, false).unwrap();
        let parent = edited.materialize_at(&["Outer".into()]).unwrap();
        let range = parent.paste_ranges("Inner").unwrap().0;
        assert!((range[0] - 0.5).abs() < 1e-6 && (range[1] - 0.75).abs() < 1e-6);
        assert_eq!(parent.pastes()[0].scale, 2.0);
        assert_eq!(edited.source_at(&path).unwrap().controls().len(), 3);
        assert!(changed_model(&root, &state, true).unwrap().source_at(&path).is_none());
    }
    #[test]
    fn nurbs_panel_tree_selection_and_menu_registration() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":500,"height":500})).unwrap();
        let root = line(&[(0.0, 0.0), (100.0, 0.0)])
            .paste_curve(line(&[(20.0, 0.0), (30.0, 20.0), (40.0, 0.0)]), "Petal".into(), PasteMode::Normalized, 1.0)
            .unwrap();
        let id = NodeId(app.run("nurbs.create", json!({"curve":root})).unwrap()["id"].as_u64().unwrap());
        pick(&mut app, id, &["Petal".into()]);
        assert_eq!(picked(&app, id), vec!["Petal".to_string()]);
        assert!(crate::state::ICON_PANELS.iter().any(|p| p.0 == ID));
        assert!(crate::state::ICON_PANEL_GROUPS.iter().any(|g| g.contains(&ID)));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| show(&mut app, ui));
        out.textures_delta.clear();
        let state: State = pstate(&ctx, ID);
        assert_eq!(state.path, vec!["Petal".to_string()]);
        assert!((state.start - 0.2).abs() < 1e-6);
        pick(&mut app, id, &[]);
        assert!(picked(&app, id).is_empty());
        assert_eq!(app.session.tool_options()["pastedMode"], json!(false));
    }
}

/// Range-strip interactions own Escape before it reaches the drawing tool.
pub(crate) fn cancel_range(app: &mut VectorcraftApp, ctx: &egui::Context) -> bool {
    if debris::settle(app, ctx) {
        return true;
    }
    if region::settle(app, ctx) {
        return true;
    }
    if color::settle(app, ctx) {
        return true;
    }
    let mut state: State = pstate(ctx, ID);
    if state.bar.is_none() {
        return false;
    }
    if app.session.active().map(|s| s.uid) != Some(state.doc) {
        state.bar = None;
        set_pstate(ctx, ID, state);
        return false;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.bar = None;
        state.last = None;
        set_pstate(ctx, ID, state);
        app.session.cancel_interaction().ok();
        app.sync_views();
        ctx.input_mut(|i| i.consume_key(i.modifiers, egui::Key::Escape));
        return true;
    }
    false
}
