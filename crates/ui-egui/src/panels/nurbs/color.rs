//! Live profile and RGBA editor. A gesture previews atomically with one Undo.
use super::*;
use vectorcraft_doc::nurbs_color::{ColorMode, ColorStroke, Domain, Profile};
const STATE: &str = "nurbs-color-editor";
#[derive(Clone, Default)]
struct Editor {
    uid: u64,
    id: Option<NodeId>,
    path: Vec<String>,
    last: serde_json::Value,
    style: ColorStroke,
    colors: Vec<[f32; 4]>,
    selected: [usize; 3],
    error: String,
    interacting: bool,
}
fn domain(ui: &mut Ui, value: &mut Domain) {
    ui.horizontal(|ui| {
        ui.selectable_value(value, Domain::Parameter, "Parameter");
        ui.selectable_value(value, Domain::Length, "Length");
    });
}
fn profile(ui: &mut Ui, id: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.push_id(id, |ui| {
        egui::CollapsingHeader::new(title).id_salt("profile-section").default_open(true).show_unindented(ui, body);
    });
}
pub(super) fn graph(ui: &mut Ui, label: &str, p: &mut Profile, low: f64, high: f64, selected: &mut usize) {
    ui.label(label);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(100.0), 85.0), egui::Sense::click());
    let r = rect.shrink(7.0);
    let to_screen = |v: [f64; 2]| egui::pos2(r.left() + r.width() * v[0] as f32, r.bottom() - r.height() * ((v[1] - low) / (high - low)) as f32);
    let to_value = |pos: egui::Pos2| {
        [((pos.x - r.left()) / r.width()).clamp(0.0, 1.0) as f64, low + (high - low) * ((r.bottom() - pos.y) / r.height()).clamp(0.0, 1.0) as f64]
    };
    ui.painter().rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    for x in [0.0, 0.5, 1.0] {
        let a = to_screen([x, low]);
        let b = to_screen([x, high]);
        ui.painter().line_segment([a, b], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    }
    if low < 0.0 {
        ui.painter().line_segment([to_screen([0.0, 0.0]), to_screen([1.0, 0.0])], egui::Stroke::new(1.0, ui.visuals().weak_text_color()));
    }
    let line: Vec<_> = (0..=120)
        .map(|i| {
            let x = i as f64 / 120.0;
            to_screen([x, p.at(x)])
        })
        .collect();
    ui.painter().add(egui::Shape::line(line, egui::Stroke::new(1.8, ui.visuals().selection.stroke.color)));
    let mut remove = None;
    let count = p.points.len();
    for i in 0..count {
        let value = p.points[i];
        let pos = to_screen(value);
        let hit = ui.interact(egui::Rect::from_center_size(pos, egui::vec2(15.0, 15.0)), ui.id().with((label, i)), egui::Sense::click_and_drag());
        if hit.clicked() || hit.drag_started() {
            *selected = i;
        }
        if hit.dragged()
            && let Some(at) = hit.interact_pointer_pos()
        {
            let mut v = to_value(at);
            v[0] = if i == 0 {
                0.0
            } else if i + 1 == count {
                1.0
            } else {
                {
                    let lo = p.points[i - 1][0];
                    let hi = p.points[i + 1][0];
                    let e = ((hi - lo) * 0.25).min(1e-5);
                    v[0].clamp(lo + e, hi - e)
                }
            };
            p.points[i] = v;
        }
        if hit.secondary_clicked() && i > 0 && i + 1 < count {
            remove = Some(i);
        }
        ui.painter().circle_filled(pos, if *selected == i { 4.5 } else { 3.5 }, ui.visuals().selection.stroke.color);
    }
    if let Some(i) = remove {
        p.points.remove(i);
        *selected = selected.saturating_sub(1);
    }
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let v = to_value(pos);
        if p.points.len() < 128 && p.points.iter().all(|p| (p[0] - v[0]).abs() > 1e-5) {
            p.points.push(v);
            p.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
            *selected = p.points.partition_point(|p| p[0] < v[0]);
        }
    }
    response.on_hover_text(
        "Drag a point. Double-click to add; right-click an interior point to remove. Changes preview immediately; Escape cancels a gesture.",
    );
    *selected = (*selected).min(p.points.len().saturating_sub(1));
    let i = *selected;
    let lo = if i == 0 { 0.0 } else { p.points[i - 1][0] };
    let hi = if i + 1 == p.points.len() { 1.0 } else { p.points[i + 1][0] };
    let e = ((hi - lo) * 0.25).min(1e-5);
    let min = if i == 0 { 0.0 } else { lo + e };
    let max = if i + 1 == p.points.len() { 1.0 } else { hi - e };
    let last = p.points.len().saturating_sub(1);
    if let Some(v) = p.points.get_mut(i) {
        ui.horizontal(|ui| {
            ui.label(format!("Point {}", i + 1));
            ui.add_enabled(i > 0 && i < last, egui::DragValue::new(&mut v[0]).range(min..=max).speed(0.005).prefix("x "));
            ui.add(egui::DragValue::new(&mut v[1]).range(low..=high).speed(0.01).prefix("y "));
        });
    }
}
fn apply_colors(root: &NurbsPath, path: &[String], colors: &[[f32; 4]]) -> Result<NurbsPath, String> {
    let mut source = root.source_at(path).ok_or("missing NURBS source")?.clone();
    if source.controls().len() != colors.len() {
        return Err("control count changed; reload colors".into());
    }
    for (i, color) in colors.iter().enumerate() {
        let mut p = *source.controls().get(i).ok_or("missing control")?;
        let rgba = color.map(f64::from);
        if p.color == rgba {
            continue;
        }
        p.color = rgba;
        source = source.set_control(i, p).map_err(|e| e.to_string())?;
    }
    root.replace_source_at(path, source).map_err(|e| e.to_string())
}
fn finish(app: &mut VectorcraftApp, state: &mut Editor, cancel: bool) {
    if !state.interacting {
        return;
    }
    let result = if cancel { app.session.cancel_interaction() } else { app.session.commit_interaction() };
    state.interacting = false;
    if cancel {
        state.id = None;
    }
    if let Err(e) = result {
        state.error = e.to_string();
    }
    app.sync_views();
}
fn preview(app: &mut VectorcraftApp, state: &mut Editor, id: NodeId, root: &NurbsPath, path: &[String]) {
    let result = state.style.validate().and_then(|_| apply_colors(root, path, &state.colors));
    let curve = match result {
        Ok(c) => c,
        Err(e) => {
            state.error = e;
            return;
        }
    };
    if !state.interacting {
        if let Err(e) = app.session.begin_interaction("Edit Colored NURBS") {
            state.error = e.to_string();
            return;
        }
        state.interacting = true;
    }
    match app.session.preview("nurbs.update", &json!({"id":id.0,"curve":curve,"color_stroke":state.style})) {
        Ok(_) => {
            state.last = json!([curve, state.style]);
            state.error.clear();
            app.sync_views();
        }
        Err(e) => {
            state.error = e.to_string();
            finish(app, state, true);
        }
    }
}
// Called from the frame's shortcut path, including when the panel has closed.
pub(super) fn settle(app: &mut VectorcraftApp, ctx: &egui::Context) -> bool {
    let mut state: Editor = pstate(ctx, STATE);
    if !state.interacting {
        return false;
    }
    if app.session.active().map(|s| s.uid) != Some(state.uid) {
        state.interacting = false;
        state.id = None;
        set_pstate(ctx, STATE, state);
        return false;
    }
    let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let target_changed = super::selected(app).is_none_or(|(id, _, _, _)| Some(id) != state.id || super::picked(app, id) != state.path);
    if cancel || target_changed || (!ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input()) {
        finish(app, &mut state, cancel);
        set_pstate(ctx, STATE, state);
        if cancel {
            ctx.input_mut(|i| i.consume_key(i.modifiers, egui::Key::Escape));
        }
    }
    cancel
}
pub(super) fn commit(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let mut state: Editor = pstate(ctx, STATE);
    finish(app, &mut state, false);
    set_pstate(ctx, STATE, state);
}
pub(super) fn show(app: &mut VectorcraftApp, ui: &mut Ui, id: NodeId, root: &NurbsPath, path: &[String], uid: u64) {
    let style = app.session.active().and_then(|s| s.doc.node(id)).and_then(|n| match &n.kind {
        NodeKind::Path { live: Some(l), .. } => match &**l {
            LiveShape::Nurbs { color_stroke, .. } => color_stroke.as_deref().cloned(),
            _ => None,
        },
        _ => None,
    });
    super::settings::show(app, ui, Some(id));
    ui.collapsing("Colored NURBS stroke", |ui| {
        let Some(current) = style else {
            if ui.button("Enable colored stroke").clicked() {
                app.run("nurbs.update", json!({"id":id.0,"curve":root,"color_stroke":ColorStroke::default()})).ok();
            }
            ui.small("Assign colors to control vertices and edit width, normal offset and cross-stroke opacity.");
            return;
        };
        let last = json!([root, current]);
        let mut state: Editor = pstate(ui.ctx(), STATE);
        if state.id != Some(id) || state.uid != uid || state.path != path || state.last != last {
            state = Editor {
                uid,
                id: Some(id),
                path: path.to_vec(),
                last,
                style: current,
                colors: root.source_at(path).map(|c| c.controls().iter().map(|p| p.color.map(|v| v as f32)).collect()).unwrap_or_default(),
                ..Default::default()
            };
        }
        let before_style = state.style.clone();
        let before_colors = state.colors.clone();
        ui.small("Live preview. One Undo per gesture; Escape cancels. White stroke paint preserves vertex colors.");
        if ui.button("Set stroke tint to white").clicked() {
            finish(app, &mut state, false);
            if let Err(e) = app.run("paint.setStroke", json!({"id":id.0,"color":"#ffffff","keepModel":true})) {
                state.error = e.to_string();
            }
        }
        ui.horizontal(|ui| {
            ui.label("Color:");
            ui.selectable_value(&mut state.style.interpolation, ColorMode::Rgb, "RGB");
            ui.selectable_value(&mut state.style.interpolation, ColorMode::Hsv, "HSV · short hue");
        });
        ui.collapsing("Vertex colors", |ui| {
            ui.label(if path.is_empty() { "Base controls" } else { "Selected pasted curve controls" });
            egui::ScrollArea::vertical().id_salt("nurbs-vertex-colors").max_height(150.0).show(ui, |ui| {
                let columns = ((ui.available_width() / 42.0).floor() as usize).clamp(1, 8);
                egui::Grid::new("vertex-color-table").num_columns(columns).spacing(egui::vec2(6.0, 4.0)).show(ui, |ui| {
                    for (i, c) in state.colors.iter_mut().enumerate() {
                        ui.vertical(|ui| {
                            ui.set_width(34.0);
                            ui.small(format!("{}", i + 1));
                            ui.color_edit_button_rgba_unmultiplied(c)
                                .on_hover_text(format!("Control {}: edit RGBA. Alt-double-click its canvas vertex for the full picker.", i + 1));
                        });
                        if (i + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
            });
        });
        profile(ui, "width-profile", "Width profile", |ui| {
            domain(ui, &mut state.style.width_domain);
            graph(ui, "Width × stroke weight", &mut state.style.width, 0.0, 2.0, &mut state.selected[0]);
        });
        profile(ui, "offset-profile", "Normal offset profile", |ui| {
            domain(ui, &mut state.style.offset_domain);
            ui.horizontal(|ui| {
                ui.label("Normal offset amount");
                ui.add(egui::DragValue::new(&mut state.style.offset_amount).range(-1e6..=1e6).speed(0.5));
            });
            graph(ui, "Normal offset", &mut state.style.offset, -1.0, 1.0, &mut state.selected[1]);
        });
        profile(ui, "opacity-profile", "Opacity across stroke", |ui| {
            graph(ui, "Opacity · left edge → right edge", &mut state.style.opacity, 0.0, 1.0, &mut state.selected[2]);
        });
        ui.label("Gradient stops along:");
        domain(ui, &mut state.style.gradient_domain);
        if state.style != before_style || state.colors != before_colors {
            preview(app, &mut state, id, root, path);
        }
        if state.interacting && !ui.input(|i| i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input() {
            finish(app, &mut state, false);
        }
        if ui.button("Disable colored stroke").clicked() {
            finish(app, &mut state, false);
            if let Err(e) = app.run("nurbs.update", json!({"id":id.0,"curve":root,"color_stroke":null})) {
                state.error = e.to_string();
            }
        }
        if !state.error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &state.error);
        }
        set_pstate(ui.ctx(), STATE, state);
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_geom::{Point, nurbs::ControlPoint};
    #[test]
    fn nurbs_color_editor_applies_shared_corner_and_preserves_pastes() {
        let root = NurbsPath::with_topology(
            vec![ControlPoint::new(Point::ZERO, 1.0), ControlPoint::new(Point::new(50.0, 50.0), 1.0), ControlPoint::new(Point::new(100.0, 0.0), 1.0)],
            vec![1],
            false,
        )
        .unwrap();
        let colors = vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.5], [0.0, 0.0, 1.0, 1.0]];
        let c = apply_colors(&root, &[], &colors).unwrap();
        assert_eq!(c.pieces()[0].controls().last().unwrap().color, [0.0, 1.0, 0.0, 0.5]);
        assert_eq!(c.pieces()[1].controls().first().unwrap().color, [0.0, 1.0, 0.0, 0.5]);
    }
    #[test]
    fn nurbs_color_graph_previews_during_drag_groups_undo_and_cancels() {
        use egui::{Event, Modifiers, PointerButton, Pos2, Rect, vec2};
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":400,"height":300})).unwrap();
        let id = NodeId(
            app.run("nurbs.create", json!({"controls":[{"x":20,"y":100},{"x":300,"y":100}],"color_stroke":{}})).unwrap()["id"].as_u64().unwrap(),
        );
        let model = |app: &mut VectorcraftApp| app.run("nurbs.get", json!({"id":id.0})).unwrap();
        let original = model(&mut app);
        let root: NurbsPath = serde_json::from_value(original["curve"].clone()).unwrap();
        let uid = app.session.active().unwrap().uid;
        let mut state = Editor { uid, id: Some(id), colors: vec![[1.0; 4]; 2], ..Default::default() };
        let ctx = egui::Context::default();
        let frame = |app: &mut VectorcraftApp, state: &mut Editor, events: Vec<Event>| {
            let mut area = Rect::NOTHING;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                ui.set_max_width(300.0);
                let old = state.style.clone();
                area = Rect::from_min_size(
                    ui.cursor().min + vec2(0.0, ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y),
                    vec2(ui.available_width(), 85.0),
                );
                graph(ui, "Width", &mut state.style.width, 0.0, 2.0, &mut state.selected[0]);
                if state.style != old {
                    preview(app, state, id, &root, &[]);
                }
                if !ui.input(|i| i.pointer.any_down()) {
                    finish(app, state, false);
                }
            });
            output.textures_delta.clear();
            area
        };
        let area = frame(&mut app, &mut state, vec![]);
        let start = Pos2::new(area.left() + 7.0, area.center().y);
        let end = start + vec2(0.0, -20.0);
        let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, &mut state, vec![Event::PointerMoved(start), button(start, true)]);
        frame(&mut app, &mut state, vec![Event::PointerMoved(end)]);
        assert!(state.interacting);
        assert!(model(&mut app)["color_stroke"]["width"]["points"][0][1].as_f64().unwrap() > 1.2, "preview must update before release");
        frame(&mut app, &mut state, vec![Event::PointerMoved(end + vec2(0.0, -5.0))]);
        frame(&mut app, &mut state, vec![button(end, false)]);
        assert!(!state.interacting);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(model(&mut app), original);
        state.style = ColorStroke::default();
        state.style.offset_amount = 17.0;
        preview(&mut app, &mut state, id, &root, &[]);
        assert_eq!(model(&mut app)["color_stroke"]["offset_amount"], 17.0);
        set_pstate(&ctx, STATE, state.clone());
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![Event::Key {
                    key: egui::Key::Escape,
                    physical_key: Some(egui::Key::Escape),
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
                ..Default::default()
            },
            |_| {
                assert!(settle(&mut app, &ctx));
            },
        );
        output.textures_delta.clear();
        assert_eq!(model(&mut app), original);
    }
}
