//! A two-handle range strip; periodic parents expose two turns, with a seam at 1.
use super::*;
use egui::{Color32, Pos2, Rect, Sense, Stroke, vec2};
#[derive(Clone)]
pub(super) struct Drag {
    root: NurbsPath,
    path: Vec<String>,
    range: [f64; 2],
    start: f32,
    kind: u8,
    periodic: bool,
}

pub(super) fn show(app: &mut VectorcraftApp, ui: &mut Ui, id: NodeId, root: &NurbsPath, path: &[String], state: &mut State) {
    let Some((name, parent_path)) = path.split_last() else { return };
    let Ok(parent) = root.materialize_at(parent_path) else { return };
    let Ok((mut span, _)) = parent.paste_ranges(name) else { return };
    let periodic = parent.is_periodic();
    let max = if periodic { 2.0 } else { 1.0 };
    if periodic && (span[0] < 0.0 || span[1] > 2.0) {
        let length = span[1] - span[0];
        span[0] = span[0].rem_euclid(1.0);
        span[1] = span[0] + length;
    }
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(80.0), 44.0), Sense::click_and_drag());
    let track = Rect::from_min_max(rect.left_center() + vec2(8.0, 2.0), rect.right_center() + vec2(-8.0, 8.0));
    let x = |u: f64| track.left() + track.width() * (u / max) as f32;
    let painter = ui.painter();
    let blue = Color32::from_rgb(35, 130, 210);
    let orange = Color32::from_rgb(215, 140, 25);
    painter.line_segment([Pos2::new(x(0.0), track.center().y), Pos2::new(x(1.0), track.center().y)], Stroke::new(5.0, blue));
    if periodic {
        painter.line_segment([Pos2::new(x(1.0), track.center().y), Pos2::new(x(2.0), track.center().y)], Stroke::new(5.0, orange));
        painter.line_segment([Pos2::new(x(1.0), rect.top() + 4.0), Pos2::new(x(1.0), rect.bottom() - 2.0)], Stroke::new(1.0, Color32::GRAY));
    }
    painter.line_segment(
        [Pos2::new(x(span[0]), track.center().y), Pos2::new(x(span[1]), track.center().y)],
        Stroke::new(9.0, Color32::from_rgba_unmultiplied(230, 230, 230, 140)),
    );
    for u in span {
        painter.circle_filled(Pos2::new(x(u), track.center().y), 5.0, Color32::WHITE);
    }
    for v in if periodic { vec![0.0, 1.0, 2.0] } else { vec![0.0, 1.0] } {
        painter.text(Pos2::new(x(v), rect.top()), egui::Align2::CENTER_TOP, format!("{v:.0}"), egui::FontId::proportional(10.0), Color32::GRAY);
    }
    if response.drag_started() {
        let at = ui.input(|i| i.pointer.press_origin()).or_else(|| response.interact_pointer_pos());
        if let Some(at) = at {
            let start = (at.x - x(span[0])).abs();
            let end = (at.x - x(span[1])).abs();
            let kind = if ui.input(|i| i.modifiers.shift) {
                2
            } else if start <= 8.0 || end <= 8.0 {
                if start <= end { 0 } else { 1 }
            } else if at.x > x(span[0]) && at.x < x(span[1]) {
                2
            } else if start <= end {
                0
            } else {
                1
            };
            if app.session.begin_interaction("Edit Pasted NURBS Range").is_ok() {
                state.bar = Some(Drag { root: root.clone(), path: path.to_vec(), range: span, start: at.x, kind, periodic });
            }
        }
    }
    if response.dragged() {
        if let (Some(d), Some(at)) = (&state.bar, response.interact_pointer_pos()) {
            let delta = (at.x - d.start) as f64 / track.width() as f64 * max;
            let mut range = d.range;
            if d.kind == 2 {
                let delta = delta.clamp(-range[0], max - range[1]);
                range = [range[0] + delta, range[1] + delta];
            } else if d.kind == 0 {
                range[0] = (range[0] + delta).clamp(if d.periodic { (range[1] - 1.0 + 1e-6).max(0.0) } else { 0.0 }, range[1] - 1e-6);
            } else {
                range[1] = (range[1] + delta).clamp(range[0] + 1e-6, if d.periodic { (range[0] + 1.0 - 1e-6).min(max) } else { max });
            }
            if let Some((name, parent_path)) = d.path.split_last() {
                let edited = d
                    .root
                    .materialize_at(parent_path)
                    .and_then(|c| c.update_paste(name, Some(range), None))
                    .and_then(|c| d.root.replace_source_at(parent_path, c));
                match edited {
                    Ok(c) => match app.session.preview("nurbs.update", &json!({"id":id.0,"curve":c,"closed":false})) {
                        Ok(_) => {
                            state.error.clear();
                            app.sync_views();
                        }
                        Err(e) => state.error = e.to_string(),
                    },
                    Err(e) => state.error = e.to_string(),
                }
            }
        }
    }
    if response.drag_stopped() && state.bar.take().is_some() {
        app.session.commit_interaction().ok();
        app.sync_views();
    }
    response.on_hover_text(if periodic {
        "0–1: first turn; 1–2: second turn. Drag handles or the selected interval across the seam."
    } else {
        "Drag the ends to resize, or the selected interval to move it."
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton};
    use vectorcraft_geom::{
        Point,
        nurbs::{ControlPoint, PasteMode},
    };
    #[test]
    fn nurbs_periodic_range_strip_moves_across_seam_with_one_undo() {
        let controls =
            vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)].into_iter().map(|(x, y)| ControlPoint::new(Point::new(x, y), 1.0)).collect();
        let base = NurbsPath::with_topology(controls, vec![], true).unwrap();
        let a = base.evaluate(0.9).unwrap();
        let b = base.evaluate(0.1).unwrap();
        let child =
            NurbsPath::from_controls(vec![ControlPoint::new(a, 1.0), ControlPoint::new(Point::new(50.0, -30.0), 1.0), ControlPoint::new(b, 1.0)])
                .unwrap();
        let root = base.paste_curve(child, "Seam".into(), PasteMode::Normalized, 1.0).unwrap();
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":500,"height":500})).unwrap();
        let id = NodeId(app.run("nurbs.create", json!({"curve":root})).unwrap()["id"].as_u64().unwrap());
        let ctx = egui::Context::default();
        let mut state = State { doc: app.session.active().unwrap().uid, ..Default::default() };
        let frame = |app: &mut VectorcraftApp, state: &mut State, events: Vec<Event>| {
            let mut area = Rect::NOTHING;
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                ui.set_max_width(300.0);
                area = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 44.0));
                if super::super::cancel_range(app, ui.ctx()) {
                    *state = pstate(ui.ctx(), ID);
                } else {
                    show(app, ui, id, &root, &["Seam".into()], state);
                }
                set_pstate(ui.ctx(), ID, state.clone());
            });
            out.textures_delta.clear();
            area
        };
        let area = frame(&mut app, &mut state, vec![]);
        let start = Pos2::new(area.center().x, area.top() + 27.0);
        let end = start + vec2((area.width() - 16.0) * 0.1, 0.0);
        let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, &mut state, vec![Event::PointerMoved(start), button(start, true)]);
        frame(&mut app, &mut state, vec![Event::PointerMoved(end)]);
        assert!(state.bar.is_some());
        frame(&mut app, &mut state, vec![button(end, false)]);
        assert!(state.bar.is_none());
        let v = app.run("nurbs.get", json!({"id":id.0})).unwrap();
        assert!((v["pasted"][0]["u_range"][0].as_f64().unwrap() - 1.1).abs() < 1e-5);
        assert!((v["pasted"][0]["u_range"][1].as_f64().unwrap() - 1.3).abs() < 1e-5);
        app.run("edit.undo", json!({})).unwrap();
        let v = app.run("nurbs.get", json!({"id":id.0})).unwrap();
        assert_eq!(v["curve"], serde_json::to_value(&root).unwrap());
        frame(&mut app, &mut state, vec![Event::PointerMoved(start), button(start, true)]);
        frame(&mut app, &mut state, vec![Event::PointerMoved(end)]);
        frame(
            &mut app,
            &mut state,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(state.bar.is_none());
        let v = app.run("nurbs.get", json!({"id":id.0})).unwrap();
        assert_eq!(v["curve"], serde_json::to_value(&root).unwrap());
    }
}
