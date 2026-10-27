//! Source-linked debris commands; all document edits and previews use Session::edit.
use super::*;
use vectorcraft_doc::nurbs_debris::{Debris, base_curve, cloud::Cloud};
fn id(s: &Session, p: &Value, cmd: &str) -> Result<NodeId> {
    if p.get("id").is_some() {
        return id_param(p, "id").ok_or_else(|| bad(cmd, "invalid debris id"));
    }
    let st = s.doc()?;
    if st.selection.objects.len() != 1 {
        return Err(bad(cmd, "select one debris object"));
    }
    st.selection.objects.first().copied().ok_or_else(|| bad(cmd, "no debris selected"))
}
fn spec(s: &Session, p: &Value, cmd: &str) -> Result<(NodeId, Debris)> {
    let id = id(s, p, cmd)?;
    let d = s.doc()?.doc.node(id).and_then(|n| n.extras().nurbs_debris.as_deref()).cloned().ok_or_else(|| bad(cmd, "not a debris object"))?;
    Ok((id, d))
}
fn validate(d: Debris, cmd: &str) -> Result<Debris> {
    serde_json::from_value(serde_json::to_value(d).map_err(|e| bad(cmd, e.to_string()))?).map_err(|e| bad(cmd, e.to_string()))
}
fn independent(doc: &vectorcraft_doc::Document, id: NodeId) -> bool {
    doc.ancestry(id)
        .is_none_or(|a| !a.into_iter().any(|ancestor| ancestor != id && doc.node(ancestor).is_some_and(|n| n.extras().nurbs_debris.is_some())))
}
pub(super) fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.debris.create";
    let st = s.doc()?;
    let (base, source) = if p.get("base").is_some() || p.get("source").is_some() {
        (id_param(p, "base").ok_or_else(|| bad(cmd, "base id required"))?, id_param(p, "source").ok_or_else(|| bad(cmd, "source id required"))?)
    } else {
        let selected = st.doc.paint_order(st.selection.objects.iter().copied());
        let [a, b] = selected.as_slice() else { return Err(bad(cmd, "select a base NURBS and one point-based source")) };
        if base_curve(&st.doc, *a).is_ok() { (*a, *b) } else { (*b, *a) }
    };
    if base == source || !independent(&st.doc, base) || !independent(&st.doc, source) {
        return Err(bad(cmd, "choose distinct independent source objects, not generated debris children"));
    }
    let mut d = Debris::new(&st.doc, base, source).map_err(|e| bad(cmd, e))?;
    for (key, slot) in [("scale", &mut d.scale), ("offset", &mut d.offset)] {
        if p.get(key).is_some() {
            *slot = f64_req(p, key, cmd)?;
        }
    }
    if p.get("parameter").is_some() {
        d.set_parameter(f64_req(p, "parameter", cmd)?).map_err(|e| bad(cmd, e))?;
    }
    if let Some(value) = p.get("display") {
        d.display = serde_json::from_value(value.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    }
    let d = validate(d, cmd)?;
    let hide = switch_or(p, "hideSource", true, cmd)?;
    let parent = st.target_parent()?;
    if hide && st.doc.node(source).is_some_and(|n| n.visible) && !st.doc.is_editable(source) {
        return Err(bad(cmd, "source is not editable; use hideSource:false to reference it without changing visibility"));
    }
    let name = p.get("name").and_then(Value::as_str).unwrap_or("NURBS Debris").to_string();
    let id = s.edit("Attach NURBS Debris", move |doc, selection| {
        let mut n = Node::group(doc.alloc_id(), vec![]);
        let id = n.id;
        n.name = Some(name);
        n.extras_mut().nurbs_debris = Some(Box::new(d));
        doc.insert(parent, usize::MAX, n)?;
        if hide && let Some(n) = doc.node_mut(source) {
            n.visible = false;
        }
        selection.set([id]);
        Ok(id)
    })?;
    get(s, &json!({"id":id.0}))
}
pub(super) fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, d) = spec(s, p, "nurbs.debris.get")?;
    let range = d.range().ok();
    let parameter = d.parameter().ok();
    let lengths = d.base.and_then(|id| base_curve(&s.doc().ok()?.doc, id).ok()).and_then(|c| {
        let frames = vectorcraft_geom::nurbs::debris::DebrisFrames::new(&c).ok()?;
        let r = range?;
        Some([frames.length_at(r[0]).ok()?, frames.length_at(r[1]).ok()?])
    });
    let phases: Vec<_> = d
        .phases
        .iter()
        .map(|phase| {
            let cloud = s.doc().ok().and_then(|st| st.doc.node(phase.source)).and_then(|n| Cloud::of(n).ok());
            json!({"key":phase.key,"source":phase.source,"vertices":cloud.as_ref().map(|c|c.vertices),"pointCount":phase.anchors.len()})
        })
        .collect();
    Ok(
        json!({"id":id.0,"base":d.base,"phases":phases,"display":d.display,"blend":d.blend,"scale":d.scale,"offset":d.offset,"parameter":parameter,"u_range":range,"l_range":lengths,"problem":d.error}),
    )
}
pub(super) fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.debris.update";
    let (id, mut d) = spec(s, p, cmd)?;
    if !s.doc()?.doc.is_editable(id) {
        return Err(bad(cmd, "debris object is locked or hidden"));
    }
    if let Some(mapping) = p.get("mapping") {
        d.map = serde_json::from_value(mapping.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    }
    if let Some(base) = p.get("base") {
        d.base = if base.is_null() { None } else { Some(id_param(p, "base").ok_or_else(|| bad(cmd, "base must be an id or null"))?) };
    }
    for (key, slot) in [("blend", &mut d.blend), ("scale", &mut d.scale), ("offset", &mut d.offset)] {
        if p.get(key).is_some() {
            *slot = f64_req(p, key, cmd)?;
        }
    }
    if p.get("parameter").is_some() {
        d.set_parameter(f64_req(p, "parameter", cmd)?).map_err(|e| bad(cmd, e))?;
    }
    if p.get("u_range").is_some() && p.get("l_range").is_some() {
        return Err(bad(cmd, "choose u_range or l_range"));
    }
    if let Some(value) = p.get("u_range").or(p.get("l_range")) {
        let mut r: [f64; 2] = serde_json::from_value(value.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        if p.get("l_range").is_some() {
            let curve = base_curve(&s.doc()?.doc, d.base.ok_or_else(|| bad(cmd, "base missing"))?).map_err(|e| bad(cmd, e))?;
            let f = vectorcraft_geom::nurbs::debris::DebrisFrames::new(&curve).map_err(|e| bad(cmd, e.to_string()))?;
            r = [
                f.parameter_at_length(r[0]).map_err(|e| bad(cmd, e.to_string()))?,
                f.parameter_at_length(r[1]).map_err(|e| bad(cmd, e.to_string()))?,
            ];
        }
        d.set_range(r).map_err(|e| bad(cmd, e))?;
    }
    if let Some(value) = p.get("display") {
        d.display = serde_json::from_value(value.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    }
    let d = validate(d, cmd)?;
    s.edit("Edit NURBS Debris", move |doc, _| {
        doc.node_mut(id).ok_or_else(|| bad(cmd, "debris missing"))?.extras_mut().nurbs_debris = Some(Box::new(d));
        Ok(())
    })?;
    get(s, &json!({"id":id.0}))
}
pub(super) fn phase_add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.debris.phase.add";
    let (id, mut d) = spec(s, p, cmd)?;
    let source = id_param(p, "source").ok_or_else(|| bad(cmd, "source id required"))?;
    let key = f64_req(p, "phase", cmd)?;
    if source == id || !independent(&s.doc()?.doc, source) {
        return Err(bad(cmd, "choose a source object, not generated debris children"));
    }
    d.add_phase(&s.doc()?.doc, source, key).map_err(|e| bad(cmd, e))?;
    let d = validate(d, cmd)?;
    let hide = switch_or(p, "hideSource", true, cmd)?;
    if hide && s.doc()?.doc.node(source).is_some_and(|n| n.visible) && !s.doc()?.doc.is_editable(source) {
        return Err(bad(cmd, "phase source is hidden or locked; use hideSource:false to retain visibility"));
    }
    s.edit("Add Debris Phase", move |doc, _| {
        doc.node_mut(id).ok_or_else(|| bad(cmd, "debris missing"))?.extras_mut().nurbs_debris = Some(Box::new(d));
        if hide && let Some(n) = doc.node_mut(source) {
            n.visible = false;
        }
        Ok(())
    })?;
    get(s, &json!({"id":id.0}))
}
pub(super) fn phase_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.debris.phase.remove";
    let (id, mut d) = spec(s, p, cmd)?;
    let key = f64_req(p, "phase", cmd)?;
    if d.phases.len() == 1 {
        return Err(bad(cmd, "a debris object needs at least one phase"));
    }
    let index = d.phases.iter().position(|p| p.key == key).ok_or_else(|| bad(cmd, "phase not found"))?;
    d.phases.remove(index);
    s.edit("Remove Debris Phase", move |doc, _| {
        doc.node_mut(id).ok_or_else(|| bad(cmd, "debris missing"))?.extras_mut().nurbs_debris = Some(Box::new(d));
        Ok(())
    })?;
    get(s, &json!({"id":id.0}))
}
pub(super) fn phase_move(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.debris.phase.move";
    let (id, mut d) = spec(s, p, cmd)?;
    let key = f64_req(p, "phase", cmd)?;
    let new = f64_req(p, "newPhase", cmd)?;
    let copy = switch_or(p, "copy", false, cmd)?;
    let mut phase = d.phases.iter().find(|p| p.key == key).cloned().ok_or_else(|| bad(cmd, "phase not found"))?;
    phase.key = new;
    d.phases.retain(|p| p.key != new && (copy || p.key != key));
    d.phases.push(phase);
    d.phases.sort_by(|a, b| a.key.total_cmp(&b.key));
    let d = validate(d, cmd)?;
    s.edit("Move Debris Phase", move |doc, _| {
        doc.node_mut(id).ok_or_else(|| bad(cmd, "debris missing"))?.extras_mut().nurbs_debris = Some(Box::new(d));
        Ok(())
    })?;
    get(s, &json!({"id":id.0}))
}
pub(super) fn source(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.debris.source";
    let (_, d) = spec(s, p, cmd)?;
    let source = if p.get("phase").is_some() {
        let key = f64_req(p, "phase", cmd)?;
        d.phases.iter().find(|p| p.key == key).map(|p| p.source)
    } else {
        d.base
    }
    .ok_or_else(|| bad(cmd, "source not found"))?;
    let visible = switch_or(p, "visible", true, cmd)?;
    let select = switch_or(p, "select", visible, cmd)?;
    s.edit("Show Debris Source", move |doc, selection| {
        let n = doc.node_mut(source).ok_or_else(|| bad(cmd, "source missing"))?;
        if n.locked {
            return Err(bad(cmd, "source is locked"));
        }
        n.visible = visible;
        if select {
            selection.set([source]);
        }
        Ok(())
    })?;
    let tool = if base_curve(&s.doc()?.doc, source).is_ok() { "nurbs" } else { "directSelection" };
    Ok(json!({"source":source.0,"tool":tool}))
}

pub(super) fn phase_fix(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.debris.phase.fix";
    let (id, mut d) = spec(s, p, cmd)?;
    let key = f64_req(p, "phase", cmd)?;
    if d.phases.iter().any(|p| p.key == key) {
        return get(s, &json!({"id":id.0}));
    }
    if d.scale.abs() < 1e-12 {
        return Err(bad(cmd, "restore a nonzero scale before fixing an interpolated phase"));
    }
    let mut sample = d.clone();
    sample.blend = key;
    let mut source = sample.build(&s.doc()?.doc).map_err(|e| bad(cmd, e))?;
    let parent = s.doc()?.target_parent()?;
    s.edit("Fix Interpolated Debris Phase", move |doc, _| {
        vectorcraft_doc::nurbs_debris::allocate(&mut source, doc);
        let source_id = source.id;
        source.visible = false;
        source.name = Some(format!("Debris phase {key}"));
        doc.insert(parent, usize::MAX, source)?;
        d.add_phase(doc, source_id, key).map_err(|e| bad(cmd, e))?;
        let d = validate(d, cmd)?;
        doc.node_mut(id).ok_or_else(|| bad(cmd, "debris missing"))?.extras_mut().nurbs_debris = Some(Box::new(d));
        Ok(())
    })?;
    get(s, &json!({"id":id.0}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_doc::{Appearance, Document, nurbs_debris::cloud::Cloud};
    use vectorcraft_geom::{Point, Rect, shapes};
    fn curve(s: &mut Session, y: f64) -> NodeId {
        NodeId(s.execute("nurbs.create", &json!({"controls":[{"x":0,"y":y},{"x":100,"y":y}]})).unwrap()["id"].as_u64().unwrap())
    }
    fn rectangle(s: &mut Session, y: f64) -> NodeId {
        s.edit("Rectangle", |doc, _| {
            let id = doc.alloc_id();
            doc.insert(None, usize::MAX, Node::path(id, shapes::rectangle(Rect::new(20.0, y, 40.0, y + 10.0)), Appearance::default_art()))?;
            Ok(id)
        })
        .unwrap()
    }
    fn setup() -> (Session, NodeId, NodeId, NodeId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width":120,"height":120})).unwrap();
        let base = curve(&mut s, 0.0);
        let source = rectangle(&mut s, 10.0);
        s.select(|_, sel| sel.set([source, base])).unwrap();
        let id = NodeId(s.execute("nurbs.debris.create", &json!({})).unwrap()["id"].as_u64().unwrap());
        (s, base, source, id)
    }
    fn points(doc: &Document, id: NodeId) -> Vec<Point> {
        Cloud::of(doc.node(id).unwrap()).unwrap().points.into_iter().map(|p| p.p).collect()
    }
    #[test]
    fn nurbs_debris_create_links_source_and_undo_is_atomic() {
        let (mut s, base, source, id) = setup();
        let doc = &s.doc().unwrap().doc;
        assert_eq!(doc.node(id).unwrap().extras().nurbs_debris.as_deref().unwrap().base, Some(base));
        assert!(!doc.node(source).unwrap().visible);
        for (a, b) in points(doc, id).iter().zip(points(doc, source)) {
            assert!(a.distance(b) < 1e-8);
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.node(id).is_none());
        assert!(s.doc().unwrap().doc.node(source).unwrap().visible);
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(!s.doc().unwrap().doc.node(source).unwrap().visible);
    }
    #[test]
    fn nurbs_debris_source_edits_missing_refs_and_replacement_recover() {
        let (mut s, base, source, id) = setup();
        let original = points(&s.doc().unwrap().doc, id);
        s.edit("Move base", |doc, _| {
            doc.node_mut(base).unwrap().transform(Affine::translate((0.0, 30.0)), false);
            Ok(())
        })
        .unwrap();
        for (a, b) in points(&s.doc().unwrap().doc, id).iter().zip(&original) {
            assert!(a.distance(*b + (0.0, 30.0)) < 1e-8);
        }
        s.edit("Move source", |doc, _| {
            doc.node_mut(source).unwrap().transform(Affine::translate((5.0, 0.0)), false);
            Ok(())
        })
        .unwrap();
        for (a, b) in points(&s.doc().unwrap().doc, id).iter().zip(&original) {
            assert!(a.distance(*b + (5.0, 30.0)) < 1e-8);
        }
        s.edit("Delete source", |doc, _| {
            doc.remove(source)?;
            Ok(())
        })
        .unwrap();
        assert!(s.doc().unwrap().doc.node(id).unwrap().children().unwrap().is_empty());
        assert!(s.execute("nurbs.debris.get", &json!({"id":id.0})).unwrap()["problem"].as_str().unwrap().contains("missing"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.execute("nurbs.debris.get", &json!({"id":id.0})).unwrap()["problem"].is_null());
        s.execute("nurbs.debris.update", &json!({"id":id.0,"base":null})).unwrap();
        assert!(s.doc().unwrap().doc.node(id).unwrap().geometric_bounds().is_none());
        let replacement = curve(&mut s, 60.0);
        s.execute("nurbs.debris.update", &json!({"id":id.0,"base":replacement.0})).unwrap();
        assert!(s.doc().unwrap().doc.node(id).unwrap().geometric_bounds().is_some());
    }
    #[test]
    fn nurbs_debris_phases_fix_move_copy_remove_and_validate_topology() {
        let (mut s, _, _, id) = setup();
        let second = rectangle(&mut s, 30.0);
        s.execute("nurbs.debris.phase.add", &json!({"id":id.0,"phase":1,"source":second.0})).unwrap();
        s.execute("nurbs.debris.update", &json!({"id":id.0,"blend":0.5,"scale":2,"offset":5})).unwrap();
        let original = points(&s.doc().unwrap().doc, id);
        s.execute("nurbs.debris.phase.fix", &json!({"id":id.0,"phase":0.5})).unwrap();
        for (a, b) in points(&s.doc().unwrap().doc, id).iter().zip(&original) {
            assert!(a.distance(*b) < 1e-7);
        }
        let before = s.doc().unwrap().doc.clone();
        s.execute("nurbs.debris.phase.fix", &json!({"id":id.0,"phase":0.5})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("nurbs.debris.phase.move", &json!({"id":id.0,"phase":1,"newPhase":2,"copy":true})).unwrap();
        s.execute("nurbs.debris.phase.move", &json!({"id":id.0,"phase":2,"newPhase":3})).unwrap();
        let read = s.execute("nurbs.debris.get", &json!({"id":id.0})).unwrap();
        assert_eq!(read["phases"][3]["key"], 3.0);
        assert_eq!(read["phases"][3]["source"], second.0);
        let incompatible = curve(&mut s, 20.0);
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("nurbs.debris.phase.add", &json!({"id":id.0,"phase":4,"source":incompatible.0})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("nurbs.debris.phase.remove", &json!({"id":id.0,"phase":3})).unwrap();
        s.execute("nurbs.debris.phase.remove", &json!({"id":id.0,"phase":1})).unwrap();
        s.execute("nurbs.debris.phase.remove", &json!({"id":id.0,"phase":0.5})).unwrap();
        assert!(s.execute("nurbs.debris.phase.remove", &json!({"id":id.0,"phase":0})).is_err());
    }
    #[test]
    fn nurbs_debris_preview_cancel_and_one_undo_restore_exact_geometry() {
        let (mut s, _, _, id) = setup();
        let original = s.doc().unwrap().doc.node(id).unwrap().clone();
        s.begin_interaction("Debris drag").unwrap();
        for u in [0.4, 0.6, 0.8] {
            s.preview("nurbs.debris.update", &json!({"id":id.0,"parameter":u})).unwrap();
        }
        s.cancel_interaction().unwrap();
        assert_eq!(s.doc().unwrap().doc.node(id).unwrap(), &original);
        s.begin_interaction("Debris drag").unwrap();
        for u in [0.4, 0.6, 0.8] {
            s.preview("nurbs.debris.update", &json!({"id":id.0,"parameter":u})).unwrap();
        }
        s.commit_interaction().unwrap();
        assert_ne!(s.doc().unwrap().doc.node(id).unwrap().kind, original.kind);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.node(id).unwrap(), &original);
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("nurbs.debris.update", &json!({"id":id.0,"mapping":[0,0]})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
    }
    #[test]
    fn nurbs_debris_native_save_keeps_links_and_rebuilds_stale_cache() {
        let (mut s, base, source, id) = setup();
        s.execute("nurbs.debris.update", &json!({"id":id.0,"scale":1.5,"offset":2})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let loaded = vectorcraft_format::load(&vectorcraft_format::save_file(doc)).unwrap();
        assert_eq!(loaded.node(id).unwrap().extras().nurbs_debris, doc.node(id).unwrap().extras().nurbs_debris);
        assert_eq!(points(&loaded, id), points(doc, id));
        let mut broken = (**doc).clone();
        broken.remove(base).unwrap();
        let loaded = vectorcraft_format::load(&vectorcraft_format::save_file(&broken)).unwrap();
        let d = loaded.node(id).unwrap();
        assert!(d.children().unwrap().is_empty());
        assert!(d.extras().nurbs_debris.as_ref().unwrap().error.is_some());
        assert!(loaded.node(source).is_some());
    }
    #[test]
    fn nurbs_debris_generated_children_cannot_be_phase_sources() {
        let (mut s, base, _, id) = setup();
        let child = s.doc().unwrap().doc.node(id).unwrap().children().unwrap()[0].id;
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("nurbs.debris.create", &json!({"base":base.0,"source":child.0})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
    }
    #[test]
    fn nurbs_debris_display_modes_preserve_links_geometry_and_native_data() {
        let (mut s, base, source, id) = setup();
        let original = points(&s.doc().unwrap().doc, id);
        for mode in ["mute", "outline", "normal"] {
            let response = s.execute("nurbs.debris.update", &json!({"id":id.0,"display":mode})).unwrap();
            assert_eq!(response["display"], mode);
            let doc = &s.doc().unwrap().doc;
            assert!(doc.is_editable(id));
            assert_eq!(points(doc, id), original);
            let children = doc.node(id).unwrap().children().unwrap();
            assert_eq!(children[0].visible, mode == "normal");
            assert_eq!(children.len(), if mode == "outline" { 2 } else { 1 });
            let loaded = vectorcraft_format::load(&vectorcraft_format::save_file(doc)).unwrap();
            assert_eq!(loaded.node(id).unwrap().extras().nurbs_debris, doc.node(id).unwrap().extras().nurbs_debris);
            assert_eq!(points(&loaded, id), original);
            assert_eq!(doc.node(id).unwrap().extras().nurbs_debris.as_ref().unwrap().base, Some(base));
            assert_eq!(doc.node(id).unwrap().extras().nurbs_debris.as_ref().unwrap().phases[0].source, source);
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("nurbs.debris.get", &json!({"id":id.0})).unwrap()["display"], "outline");
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("nurbs.debris.update", &json!({"id":id.0,"display":"invalid"})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
        let spec = s.doc().unwrap().doc.node(id).unwrap().extras().nurbs_debris.as_ref().unwrap();
        let mut legacy = serde_json::to_value(spec).unwrap();
        legacy.as_object_mut().unwrap().remove("display");
        let legacy: Debris = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.display, vectorcraft_doc::nurbs_debris::DisplayMode::Normal);
    }
    #[test]
    fn nurbs_debris_linked_debris_ignores_source_display_mode() {
        let (mut s, base, _, id) = setup();
        let chain =
            NodeId(s.execute("nurbs.debris.create", &json!({"base":base.0,"source":id.0,"hideSource":false})).unwrap()["id"].as_u64().unwrap());
        let original = points(&s.doc().unwrap().doc, chain);
        for mode in ["outline", "mute", "normal"] {
            s.execute("nurbs.debris.update", &json!({"id":id.0,"display":mode})).unwrap();
            assert!(s.execute("nurbs.debris.get", &json!({"id":chain.0})).unwrap()["problem"].is_null());
            for (a, b) in points(&s.doc().unwrap().doc, chain).iter().zip(&original) {
                assert!(a.distance(*b) < 1e-8);
            }
        }
    }
}
