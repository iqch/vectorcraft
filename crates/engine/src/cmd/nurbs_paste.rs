//! Pasted NURBS commands shared by the control API and the NURBS panel.
use super::*;
use vectorcraft_geom::nurbs::{NurbsPath, PasteMode};

fn mode(p: &Value, cmd: &str) -> Result<PasteMode> {
    match p.get("mode") {
        None => Ok(PasteMode::Normalized),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad(cmd, "mode must be u, p or l")),
    }
}
fn source(node: &Node) -> Option<(&NurbsPath, Affine)> {
    let NodeKind::Path { live: Some(live), .. } = &node.kind else { return None };
    let LiveShape::Nurbs { curve, xf, .. } = &**live else { return None };
    Some((curve, *xf))
}
fn install(doc: &mut vectorcraft_doc::Document, id: NodeId, curve: NurbsPath, cmd: &str) -> Result<()> {
    let bp = curve.to_bezpath(0.05).map_err(|e| bad(cmd, e.to_string()))?;
    let node = doc.node_mut(id).ok_or_else(|| bad(cmd, "object missing"))?;
    let NodeKind::Path { path, live: Some(live), .. } = &mut node.kind else { return Err(bad(cmd, "not a NURBS curve")) };
    let LiveShape::Nurbs { curve: original, preview, xf, .. } = &mut **live else { return Err(bad(cmd, "not a NURBS curve")) };
    *preview = PathData::from_bezpath(&bp);
    *path = preview.transformed(*xf);
    *original = curve;
    Ok(())
}
pub(super) fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.paste";
    let st = s.doc()?;
    let group = if p.get("id").is_some() {
        id_param(p, "id").ok_or_else(|| bad(cmd, "invalid group id"))?
    } else {
        if st.selection.objects.len() != 1 {
            return Err(bad(cmd, "select one ordinary group"));
        }
        *st.selection.objects.first().ok_or_else(|| bad(cmd, "select one ordinary group"))?
    };
    let node = st.doc.node(group).ok_or_else(|| bad(cmd, "group missing"))?;
    let NodeKind::Group { children, clip: false } = &node.kind else { return Err(bad(cmd, "expected an ordinary, non-clipping group")) };
    if node.locked {
        return Err(bad(cmd, "group is locked"));
    }
    // Stored child order is bottom-to-top, independent of click/selection order.
    // Nested groups are not recursively consumed.
    let curves: Vec<_> = children.iter().filter_map(|n| source(n).map(|(c, xf)| (n.id, c.clone(), xf, n.locked, n.name.clone()))).collect();
    if curves.len() < 2 {
        return Err(bad(cmd, "group needs at least two direct NURBS curves"));
    }
    if curves.iter().any(|c| c.3) {
        return Err(bad(cmd, "a NURBS member is locked"));
    }
    let (base_id, mut result, base_xf, _, _) = curves.first().cloned().ok_or_else(|| bad(cmd, "no base"))?;
    if !base_xf.determinant().is_finite() || base_xf.determinant() == 0.0 {
        return Err(bad(cmd, "base transform is singular"));
    }
    let mode = mode(p, cmd)?;
    let scale = p.get("scale").map(|_| f64_req(p, "scale", cmd)).transpose()?.unwrap_or(1.0);
    let mut consumed = Vec::new();
    let mut names = Vec::new();
    for (id, curve, xf, _, original_name) in curves.iter().skip(1) {
        let child = curve.transformed(base_xf.inverse() * *xf).map_err(|e| bad(cmd, e.to_string()))?;
        let mut stem = original_name.as_deref().filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or_else(|| format!("curve-{}", id.0));
        while stem.len() > 256 {
            stem.pop();
        }
        let mut name = stem.clone();
        let mut suffix = 2usize;
        while result.pastes().iter().any(|p| p.name == name) {
            let ending = format!(" ({suffix})");
            let mut prefix = stem.clone();
            while prefix.len() + ending.len() > 256 {
                prefix.pop();
            }
            name = prefix + &ending;
            suffix += 1;
        }
        result = result.paste_curve(child, name.clone(), mode, scale).map_err(|e| bad(cmd, format!("{}: {e}", name)))?;
        consumed.push(*id);
        names.push(name);
    }
    // Validate rendering before any document mutation; s.edit rolls back every failure.
    let _ = result.to_bezpath(0.05).map_err(|e| bad(cmd, e.to_string()))?;
    let ignored: Vec<_> = children.iter().filter(|n| source(n).is_none()).map(|n| n.id.0).collect();
    s.edit("Paste NURBS Group", move |doc, selection| {
        install(doc, base_id, result, cmd)?;
        for id in consumed {
            doc.remove(id)?;
        }
        selection.set([base_id]);
        Ok(())
    })?;
    Ok(json!({"id":base_id.0,"group":group.0,"pasted":names,"ignored":ignored}))
}
pub(super) fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, curve) = nurbs_source(s, p, "nurbs.get")?;
    let node = s.doc()?.doc.node(id).ok_or_else(|| bad("nurbs.get", "object missing"))?;
    let (_, xf) = source(node).ok_or_else(|| bad("nurbs.get", "not a NURBS curve"))?;
    let pasted = curve
        .pastes()
        .iter()
        .map(|v| {
            let (u_range, l_range) = curve.paste_ranges(&v.name).map_err(|e| bad("nurbs.get", e.to_string()))?;
            Ok(json!({"name":v.name,"mode":v.mode,"scale":v.scale,"enabled":v.enabled,"u_range":u_range,"l_range":l_range}))
        })
        .collect::<Result<Vec<_>>>()?;
    let color_stroke = match &node.kind {
        NodeKind::Path { live: Some(live), .. } => match &**live {
            LiveShape::Nurbs { color_stroke, .. } => color_stroke.as_deref(),
            _ => None,
        },
        _ => None,
    };
    Ok(json!({"id":id.0,"curve":curve,"transform":xf.as_coeffs(),"pasted":pasted,"color_stroke":color_stroke}))
}
fn range(p: &Value, key: &str, cmd: &str) -> Result<Option<[f64; 2]>> {
    let Some(v) = p.get(key) else { return Ok(None) };
    let v = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, format!("{key} needs two numbers")))?;
    let a = v[0].as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "invalid range"))?;
    let b = v[1].as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "invalid range"))?;
    Ok(Some([a, b]))
}
pub(super) fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.paste.update";
    let (id, curve) = nurbs_source(s, p, cmd)?;
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "missing name"))?;
    let u = range(p, "u_range", cmd)?;
    let l = range(p, "l_range", cmd)?;
    if u.is_some() && l.is_some() {
        return Err(bad(cmd, "use u_range or l_range, not both"));
    }
    let u = if let Some(l) = l { Some(curve.length_range_to_parameters(l).map_err(|e| bad(cmd, e.to_string()))?) } else { u };
    let scale = p.get("scale").map(|_| f64_req(p, "scale", cmd)).transpose()?;
    let enabled = p.get("enabled").map(|_| switch_or(p, "enabled", true, cmd)).transpose()?;
    let curve = if enabled == Some(false) { curve.set_paste_enabled(name, false).map_err(|e| bad(cmd, e.to_string()))? } else { curve };
    let curve = curve.update_paste(name, u, scale).map_err(|e| bad(cmd, e.to_string()))?;
    let curve = if enabled == Some(true) { curve.set_paste_enabled(name, true).map_err(|e| bad(cmd, e.to_string()))? } else { curve };
    nurbs_update(s, &json!({"id":id.0,"curve":curve,"closed":false}))
}
pub(super) fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.paste.remove";
    let (id, curve) = nurbs_source(s, p, cmd)?;
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "missing name"))?;
    let curve = curve.remove_paste(name).map_err(|e| bad(cmd, e.to_string()))?;
    nurbs_update(s, &json!({"id":id.0,"curve":curve,"closed":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(v: Value) -> NodeId {
        NodeId(v["id"].as_u64().unwrap())
    }
    fn base(s: &mut Session) -> NodeId {
        id(s.execute("nurbs.create", &json!({"controls":[{"x":0,"y":0},{"x":100,"y":0}]})).unwrap())
    }
    fn bump(s: &mut Session, a: f64, b: f64) -> NodeId {
        id(s.execute("nurbs.create", &json!({"controls":[{"x":a,"y":0},{"x":(a+b)/2.0,"y":20},{"x":b,"y":0}]})).unwrap())
    }
    fn setup(overlap: bool) -> (Session, NodeId, NodeId, NodeId, Vec<NodeId>) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width":500,"height":500})).unwrap();
        let ignored = id(s.execute("shape.rectangle", &json!({"x":0,"y":100,"width":20,"height":20})).unwrap());
        let base = base(&mut s);
        let first = bump(&mut s, 20.0, 40.0);
        let second = bump(&mut s, if overlap { 30.0 } else { 60.0 }, if overlap { 50.0 } else { 80.0 });
        // Deliberately reverse the selection. Group command uses stored document order.
        s.doc_mut().unwrap().selection.set([second, first, base, ignored]);
        let group = id(s.execute("object.group", &json!({})).unwrap());
        (s, group, base, ignored, vec![first, second])
    }
    #[test]
    fn nurbs_paste_group_api_roundtrip_history_and_ignored_objects() {
        let (mut s, group, base, ignored, children) = setup(false);
        let original = s.doc().unwrap().doc.node(group).unwrap().clone();
        let appearance = s.doc().unwrap().doc.node(base).unwrap().appearance.clone();
        let result = s.execute("nurbs.paste", &json!({})).unwrap();
        assert_eq!(result["id"], json!(base.0));
        assert_eq!(result["ignored"], json!([ignored.0]));
        assert!(children.iter().all(|id| s.doc().unwrap().doc.node(*id).is_none()));
        assert_eq!(
            s.doc().unwrap().doc.node(ignored),
            match &original.kind {
                NodeKind::Group { children, .. } => children.iter().find(|n| n.id == ignored).map(|n| n.as_ref()),
                _ => None,
            }
        );
        assert_eq!(s.doc().unwrap().doc.node(base).unwrap().appearance, appearance);
        let info = s.execute("nurbs.get", &json!({"id":base.0})).unwrap();
        assert_eq!(info["pasted"].as_array().unwrap().len(), 2);
        let saved = vectorcraft_format::save_file(&s.doc().unwrap().doc);
        let reopened = vectorcraft_format::load(&saved).unwrap();
        assert_eq!(reopened.node(group), s.doc().unwrap().doc.node(group));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.node(group).unwrap(), &original);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.execute("nurbs.get", &json!({"id":base.0})).unwrap()["curve"], info["curve"]);
        // Full-model create and overwrite use the same serialized representation.
        let copy = id(s.execute("nurbs.create", &json!({"curve":info["curve"]})).unwrap());
        assert_eq!(s.execute("nurbs.get", &json!({"id":copy.0})).unwrap()["curve"], info["curve"]);
        s.execute("nurbs.update", &json!({"id":copy.0,"curve":info["curve"]})).unwrap();
        let name = info["pasted"][0]["name"].as_str().unwrap();
        s.execute("nurbs.paste.update", &json!({"id":copy.0,"name":name,"u_range":[0.1,0.3],"scale":2.0})).unwrap();
        let moved = s.execute("nurbs.get", &json!({"id":copy.0})).unwrap();
        assert_eq!(moved["pasted"][0]["scale"], json!(2.0));
        s.execute("nurbs.paste.remove", &json!({"id":copy.0,"name":name})).unwrap();
        assert_eq!(s.execute("nurbs.get", &json!({"id":copy.0})).unwrap()["pasted"].as_array().unwrap().len(), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("nurbs.get", &json!({"id":copy.0})).unwrap(), moved);
    }
    #[test]
    fn nurbs_paste_rejects_overlap_and_invalid_updates_atomically() {
        let (mut s, group, base, _, children) = setup(true);
        let original = s.doc().unwrap().doc.node(group).unwrap().clone();
        assert!(s.execute("nurbs.paste", &json!({"id":group.0})).is_err());
        assert_eq!(s.doc().unwrap().doc.node(group).unwrap(), &original);
        assert!(children.iter().all(|id| s.doc().unwrap().doc.node(*id).is_some()));
        assert!(s.execute("nurbs.paste", &json!({"id":base.0})).is_err());
        let (mut s, group, base, _, _) = setup(false);
        s.execute("nurbs.paste", &json!({"id":group.0,"mode":"l"})).unwrap();
        let original = s.execute("nurbs.get", &json!({"id":base.0})).unwrap();
        let name = original["pasted"][0]["name"].as_str().unwrap();
        for params in [
            json!({"id":base.0,"name":name,"u_range":[0.6,0.9]}),
            json!({"id":base.0,"name":name,"l_range":[-10.0,30.0]}),
            json!({"id":base.0,"name":name,"u_range":[0.1,0.3],"l_range":[10.0,30.0]}),
            json!({"id":base.0,"name":"missing","scale":1.0}),
        ] {
            assert!(s.execute("nurbs.paste.update", &params).is_err());
            assert_eq!(s.execute("nurbs.get", &json!({"id":base.0})).unwrap(), original);
        }
        s.execute("nurbs.paste.update", &json!({"id":base.0,"name":name,"l_range":[10.0,30.0]})).unwrap();
    }
    #[test]
    fn nurbs_paste_respects_member_transforms_and_rejects_periodic_child() {
        let (mut s, group, base, _, children) = setup(false);
        // The children are shifted together in document space by their live transform.
        s.edit("Test transform", |doc, _| {
            for id in [base, children[0], children[1]] {
                let node = doc.node_mut(id).unwrap();
                let NodeKind::Path { path, live: Some(live), .. } = &mut node.kind else { panic!("path") };
                let LiveShape::Nurbs { preview, xf, .. } = &mut **live else { panic!("NURBS") };
                *xf = Affine::translate((12.0, 24.0)) * Affine::scale(2.0);
                *path = preview.transformed(*xf);
            }
            Ok(())
        })
        .unwrap();
        s.execute("nurbs.paste", &json!({"id":group.0})).unwrap();
        let info = s.execute("nurbs.get", &json!({"id":base.0})).unwrap();
        let curve: NurbsPath = serde_json::from_value(info["curve"].clone()).unwrap();
        assert!(curve.evaluate_composed(0.3).unwrap().distance(Point::new(30.0, 10.0)) < 1e-6);
        assert_eq!(info["transform"], json!([2.0, 0.0, 0.0, 2.0, 12.0, 24.0]));
        let (mut s, group, _, _, children) = setup(false);
        s.execute("nurbs.periodic", &json!({"id":children[0].0,"periodic":true})).unwrap();
        let original = s.doc().unwrap().doc.node(group).unwrap().clone();
        assert!(s.execute("nurbs.paste", &json!({"id":group.0})).is_err());
        assert_eq!(s.doc().unwrap().doc.node(group).unwrap(), &original);
    }
    #[test]
    fn nurbs_pasted_names_inherit_and_disambiguate_original_labels() {
        let (mut s, group, base, _, children) = setup(false);
        s.edit("Name source curves", |doc, _| {
            doc.node_mut(base).unwrap().name = Some("Основа".into());
            for child in &children {
                doc.node_mut(*child).unwrap().name = Some("Лепесток".into());
            }
            Ok(())
        })
        .unwrap();
        let result = s.execute("nurbs.paste", &json!({"id":group.0})).unwrap();
        assert_eq!(result["pasted"], json!(["Лепесток", "Лепесток (2)"]));
        assert_eq!(s.doc().unwrap().doc.node(base).unwrap().name.as_deref(), Some("Основа"));
        let saved = vectorcraft_format::save_file(&s.doc().unwrap().doc);
        let reopened = vectorcraft_format::load(&saved).unwrap();
        let (curve, _) = source(reopened.node(base).unwrap()).unwrap();
        assert_eq!(curve.pastes()[0].name, "Лепесток");
        assert_eq!(curve.pastes()[1].name, "Лепесток (2)");
        s.execute("edit.undo", &json!({})).unwrap();
        for child in children {
            assert_eq!(s.doc().unwrap().doc.node(child).unwrap().name.as_deref(), Some("Лепесток"));
        }
    }
    #[test]
    fn nurbs_pasted_canvas_select_drag_cancel_remove_and_history() {
        use crate::tooling::ViewInfo;
        use vectorcraft_tools::{Mods, PointerEvent, PointerKind, ToolKey};
        let (mut s, group, base, _, _) = setup(false);
        s.execute("nurbs.paste", &json!({"id":group.0})).unwrap();
        let initial = s.execute("nurbs.get", &json!({"id":base.0})).unwrap();
        let view = ViewInfo::default();
        s.select_tool("nurbs", view).unwrap();
        s.set_tool_option("pastedMode", &json!(true));
        let pointer = |s: &mut Session, kind, x, y| s.pointer(&PointerEvent::new(kind, x, y), view).unwrap();
        pointer(&mut s, PointerKind::Down, 30.0, 10.0);
        let options = s.tool_options();
        assert_eq!(options["pastedTarget"], json!(base.0));
        assert_eq!(options["pastedPath"], json!([initial["pasted"][0]["name"]]));
        pointer(&mut s, PointerKind::Drag, 40.0, 10.0);
        pointer(&mut s, PointerKind::Up, 40.0, 10.0);
        let moved = s.execute("nurbs.get", &json!({"id":base.0})).unwrap();
        assert!((moved["pasted"][0]["u_range"][0].as_f64().unwrap() - 0.3).abs() < 1e-6);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("nurbs.get", &json!({"id":base.0})).unwrap()["curve"], initial["curve"]);
        pointer(&mut s, PointerKind::Down, 30.0, 10.0);
        pointer(&mut s, PointerKind::Drag, 40.0, 10.0);
        s.tool_key(ToolKey::Escape, Mods::default(), view).unwrap();
        pointer(&mut s, PointerKind::Up, 40.0, 10.0);
        assert_eq!(s.execute("nurbs.get", &json!({"id":base.0})).unwrap()["curve"], initial["curve"]);
        // A rejected overlap leaves the last valid model intact.
        pointer(&mut s, PointerKind::Down, 30.0, 10.0);
        pointer(&mut s, PointerKind::Drag, 70.0, 10.0);
        pointer(&mut s, PointerKind::Up, 70.0, 10.0);
        assert_eq!(s.execute("nurbs.get", &json!({"id":base.0})).unwrap()["curve"], initial["curve"]);
        s.tool_key(ToolKey::Delete, Mods::default(), view).unwrap();
        assert_eq!(s.execute("nurbs.get", &json!({"id":base.0})).unwrap()["pasted"].as_array().unwrap().len(), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("nurbs.get", &json!({"id":base.0})).unwrap()["curve"], initial["curve"]);
    }
}

pub(super) fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.paste.duplicate";
    let (id, curve) = nurbs_source(s, p, cmd)?;
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "missing name"))?;
    let new_name = p.get("newName").map(|v| v.as_str().ok_or_else(|| bad(cmd, "newName must be a string"))).transpose()?;
    let (curve, name) = curve.duplicate_paste(name, new_name).map_err(|e| bad(cmd, e.to_string()))?;
    nurbs_update(s, &json!({"id":id.0,"curve":curve,"closed":false}))?;
    Ok(json!({"id":id.0,"name":name,"enabled":false}))
}

#[cfg(test)]
mod enable_tests {
    use super::*;
    #[test]
    fn nurbs_paste_duplicate_disable_api_and_native_history() {
        use vectorcraft_geom::{
            Point,
            nurbs::{ControlPoint, PasteMode},
        };
        let curve = |p: &[(f64, f64)]| NurbsPath::from_controls(p.iter().map(|&(x, y)| ControlPoint::new(Point::new(x, y), 1.0)).collect()).unwrap();
        let root = curve(&[(0.0, 0.0), (100.0, 0.0)])
            .paste_curve(curve(&[(20.0, 0.0), (30.0, 20.0), (40.0, 0.0)]), "Petal".into(), PasteMode::Normalized, 1.0)
            .unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"width":500,"height":500})).unwrap();
        let id = s.execute("nurbs.create", &json!({"curve":root})).unwrap()["id"].as_u64().unwrap();
        let duplicated = s.execute("nurbs.paste.duplicate", &json!({"id":id,"name":"Petal"})).unwrap();
        assert_eq!(duplicated["name"], json!("Petal (2)"));
        assert_eq!(duplicated["enabled"], json!(false));
        assert!(s.execute("nurbs.paste.update", &json!({"id":id,"name":"Petal (2)","enabled":true})).is_err());
        s.execute("nurbs.paste.update", &json!({"id":id,"name":"Petal (2)","u_range":[0.6,0.8],"enabled":true})).unwrap();
        s.execute("nurbs.paste.update", &json!({"id":id,"name":"Petal","enabled":false})).unwrap();
        let current = s.execute("nurbs.get", &json!({"id":id})).unwrap();
        assert_eq!(current["pasted"][0]["enabled"], json!(false));
        assert_eq!(current["pasted"][1]["enabled"], json!(true));
        let saved = vectorcraft_format::save_file(&s.doc().unwrap().doc);
        let reopened = vectorcraft_format::load(&saved).unwrap();
        let (curve, _) = source(reopened.node(NodeId(id)).unwrap()).unwrap();
        assert!(!curve.pastes()[0].enabled);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("nurbs.get", &json!({"id":id})).unwrap()["pasted"][0]["enabled"], json!(true));
    }
}
