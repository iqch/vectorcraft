//! Full RGBA picker for an editable NURBS control, with one preview transaction.
use super::{DialogSpec, form};
use crate::{VectorcraftApp, state::Dialog};
use serde_json::{Value, json};
use vectorcraft_doc::{LiveShape, NodeId, NodeKind, nurbs_color::ColorStroke};
use vectorcraft_geom::nurbs::NurbsPath;
pub(super) const KIND: &str = "nurbsVertexColor";
pub(super) const SPEC: DialogSpec =
    DialogSpec { heading: |_| "NURBS vertex color".into(), body, confirm, preview: true, min_width: 270.0, ..DialogSpec::FORM };
pub(super) fn open(app: &mut VectorcraftApp, p: &Value) -> Result<(), String> {
    let id = p.get("id").and_then(Value::as_u64).ok_or("missing NURBS id")?;
    let index = p.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).ok_or("missing vertex index")?;
    let st = app.session.active().ok_or("no document")?;
    let node = st.doc.node(NodeId(id)).ok_or("missing object")?;
    let NodeKind::Path { live: Some(live), .. } = &node.kind else {
        return Err("select an editable NURBS".into());
    };
    let LiveShape::Nurbs { curve, color_stroke, .. } = &**live else {
        return Err("select an editable NURBS".into());
    };
    let path: Vec<String> = serde_json::from_value(p.get("path").cloned().unwrap_or(json!([]))).map_err(|e| e.to_string())?;
    let rgba = curve.source_at(&path).ok_or("missing pasted source")?.controls().get(index).ok_or("missing control")?.color.map(|v| v as f32);
    let fields = json!({"id":id,"index":index,"uid":st.uid,"curve":curve,"rgba":rgba,"original":rgba,"path":path,"color_stroke":color_stroke});
    app.ui.dialog = Some(Dialog::new(KIND, fields));
    Ok(())
}
fn params(app: &VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    if app.session.active().map(|s| s.uid) != d.fields.get("uid").and_then(Value::as_u64) {
        return Err("document changed".into());
    }
    let id = d.fields.get("id").and_then(Value::as_u64).ok_or("missing id")?;
    let index = d.fields.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).ok_or("missing index")?;
    let mut curve: NurbsPath = serde_json::from_value(d.fields.get("curve").cloned().ok_or("missing curve")?).map_err(|e| e.to_string())?;
    let rgba: [f32; 4] = serde_json::from_value(d.fields.get("rgba").cloned().ok_or("missing RGBA")?).map_err(|e| e.to_string())?;
    if rgba.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
        return Err("RGBA must be in [0,1]".into());
    }
    let changed = d.fields.get("rgba") != d.fields.get("original");
    if changed {
        let path: Vec<String> = serde_json::from_value(d.fields.get("path").cloned().unwrap_or(json!([]))).map_err(|e| e.to_string())?;
        let source = curve.source_at(&path).ok_or("missing pasted source")?;
        let mut cp = *source.controls().get(index).ok_or("missing control")?;
        cp.color = rgba.map(f64::from);
        let source = source.set_control(index, cp).map_err(|e| e.to_string())?;
        curve = curve.replace_source_at(&path, source).map_err(|e| e.to_string())?;
    }
    let mut style = d.fields.get("color_stroke").cloned().unwrap_or(Value::Null);
    if changed && style.is_null() {
        style = json!(ColorStroke::default());
    }
    Ok(json!({"id":id,"curve":curve,"color_stroke":style}))
}
fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    ui.label(format!("Control {}", d.fields.get("index").and_then(Value::as_u64).unwrap_or(0) + 1));
    let Ok(mut rgba) = serde_json::from_value::<[f32; 4]>(d.fields.get("rgba").cloned().unwrap_or(Value::Null)) else {
        ui.label("Invalid RGBA");
        return false;
    };
    let old = rgba;
    let mut hsva = egui::ecolor::Hsva::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    if egui::color_picker::color_picker_hsva_2d(ui, &mut hsva, egui::color_picker::Alpha::OnlyBlend) {
        rgba = hsva.to_rgba_unmultiplied();
    }
    for (i, label) in ["R", "G", "B", "Alpha"].iter().enumerate() {
        ui.add(egui::Slider::new(&mut rgba[i], 0.0..=1.0).text(*label));
    }
    if rgba != old {
        d.fields.insert("rgba".into(), json!(rgba));
    }
    let dirty = d.fields.get("rgba") != d.fields.get("original");
    if !dirty && !app.session.in_interaction() {
        return false;
    }
    match params(app, d) {
        Ok(p) => {
            form::preview_now(app, d, "NURBS Vertex Color", "nurbs.update", p);
        }
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }
    false
}
fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let p = params(app, d)?;
    if d.fields.get("rgba") == d.fields.get("original") && !app.session.in_interaction() {
        app.ui.dialog = None;
        return Ok(Value::Null);
    }
    form::commit_preview(app, "nurbs.update", p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nurbs_vertex_picker_preview_cancel_confirm_and_undo_preserve_corner_weight() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":400,"height":300})).unwrap();
        let id = app
            .run(
                "nurbs.create",
                json!({"controls":[{"x":20,"y":100},{"x":150,"y":20,"weight":1.7},{"x":300,"y":100}],"corners":[1],"color_stroke":{}}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let model = |app: &mut VectorcraftApp| app.run("nurbs.get", json!({"id":id})).unwrap();
        let original = model(&mut app);
        open(&mut app, &json!({"id":id,"index":1})).unwrap();
        let mut d = app.ui.dialog.clone().unwrap();
        d.fields.insert("rgba".into(), json!([0.2, 0.5, 0.7, 0.4]));
        let p = params(&app, &d).unwrap();
        form::preview_now(&mut app, &mut d, "NURBS Vertex Color", "nurbs.update", p);
        app.ui.dialog = Some(d);
        assert_ne!(model(&mut app), original);
        super::super::cancel(&mut app);
        assert_eq!(model(&mut app), original);
        open(&mut app, &json!({"id":id,"index":1})).unwrap();
        let mut d = app.ui.dialog.clone().unwrap();
        d.fields.insert("rgba".into(), json!([0.0, 0.0, 1.0, 0.5]));
        confirm(&mut app, &d).unwrap();
        let modified = model(&mut app);
        let c: NurbsPath = serde_json::from_value(modified["curve"].clone()).unwrap();
        assert_eq!(c.controls()[1].color, [0.0, 0.0, 1.0, 0.5]);
        assert_eq!(c.controls()[1].weight, 1.7);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(model(&mut app), original);
        open(&mut app, &json!({"id":id,"index":1})).unwrap();
        let mut d = app.ui.dialog.clone().unwrap();
        d.fields.insert("rgba".into(), json!([1.0, 0.0, 0.0, 2.0]));
        assert!(confirm(&mut app, &d).is_err());
        assert_eq!(model(&mut app), original);
    }
    #[test]
    fn nurbs_vertex_picker_edits_pasted_source_without_moving_bindings() {
        use vectorcraft_geom::{
            Point,
            nurbs::{ControlPoint, PasteMode},
        };
        let cp = |x, y| ControlPoint::new(Point::new(x, y), 1.0);
        let base = NurbsPath::from_controls(vec![cp(20.0, 100.0), cp(300.0, 100.0)]).unwrap();
        let child = NurbsPath::from_controls(vec![cp(80.0, 100.0), cp(160.0, 20.0), cp(240.0, 100.0)]).unwrap();
        let root = base.paste_curve(child, "Ink".into(), PasteMode::Normalized, 1.0).unwrap();
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":400,"height":300})).unwrap();
        let id = app.run("nurbs.create", json!({"curve":root,"color_stroke":{}})).unwrap()["id"].as_u64().unwrap();
        open(&mut app, &json!({"id":id,"path":["Ink"],"index":1})).unwrap();
        let mut d = app.ui.dialog.clone().unwrap();
        d.fields.insert("rgba".into(), json!([0.0, 1.0, 0.0, 0.25]));
        confirm(&mut app, &d).unwrap();
        let result = app.run("nurbs.get", json!({"id":id})).unwrap();
        let edited: NurbsPath = serde_json::from_value(result["curve"].clone()).unwrap();
        assert_eq!(edited.source_at(&["Ink".into()]).unwrap().controls()[1].color, [0.0, 1.0, 0.0, 0.25]);
        assert_eq!(edited.controls(), root.controls());
        assert_eq!(edited.pastes()[0].anchors, root.pastes()[0].anchors);
    }
}
