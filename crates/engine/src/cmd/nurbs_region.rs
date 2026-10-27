//! Commands for live fills linked to two periodic NURBS sources.
use super::*;
use vectorcraft_color::Paint;
use vectorcraft_doc::nurbs_region::Region;

pub(super) fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.region.create";
    let st = s.doc()?;
    let (outer, inner) = if p.get("outer").is_some() || p.get("inner").is_some() {
        (id_param(p, "outer").ok_or_else(|| bad(cmd, "outer id required"))?, id_param(p, "inner").ok_or_else(|| bad(cmd, "inner id required"))?)
    } else {
        if st.selection.objects.len() != 2 {
            return Err(bad(cmd, "select exactly two periodic NURBS curves"));
        }
        let ids = st.doc.paint_order(st.selection.objects.iter().copied());
        let [a, b] = ids.as_slice() else { return Err(bad(cmd, "select two different source curves")) };
        let area = |id| st.doc.node(id).and_then(|n| n.geometric_bounds()).map(|r| r.area()).unwrap_or(0.0);
        if area(*a) >= area(*b) { (*a, *b) } else { (*b, *a) }
    };
    let mut spec = Region::new(outer, inner);
    if let Some(profile) = p.get("profile") {
        spec = serde_json::from_value(json!({"outer":outer,"inner":inner,"profile":profile})).map_err(|e| bad(cmd, e.to_string()))?;
    }
    if let Some(e) = spec.problem(&st.doc) {
        return Err(bad(cmd, e));
    }
    if !st.doc.is_editable(outer) || !st.doc.is_editable(inner) {
        return Err(bad(cmd, "source curves must be editable"));
    }
    let parent = st.target_parent()?;
    let clear_sources = switch_or(p, "clearSourcePaint", true, cmd)?;
    let fill = if s.paint.fill.is_none() { Paint::solid(vectorcraft_color::Color::BLACK) } else { s.paint.fill.clone() };
    let look = s.new_art_look(fill, Paint::None, 0.0);
    let id = s.edit("Create NURBS Region", move |doc, selection| {
        let id = doc.alloc_id();
        let mut node = Node::path(id, PathData::default(), Appearance::default());
        look.apply(doc, &mut node);
        node.name = Some("NURBS Region".into());
        node.extras_mut().nurbs_region = Some(Box::new(spec));
        if clear_sources {
            for source in [outer, inner] {
                if let Some(n) = doc.node_mut(source) {
                    for item in &mut n.appearance.items {
                        *item.paint_mut() = Paint::None;
                    }
                }
            }
        }
        doc.insert(parent, usize::MAX, node)?;
        selection.set([id]);
        Ok(id)
    })?;
    Ok(json!({"id":id.0,"outer":outer.0,"inner":inner.0}))
}
fn selected_id(s: &Session, p: &Value, cmd: &str) -> Result<NodeId> {
    if p.get("id").is_some() {
        return id_param(p, "id").ok_or_else(|| bad(cmd, "invalid object id"));
    }
    let st = s.doc()?;
    if st.selection.objects.len() != 1 {
        return Err(bad(cmd, "select one NURBS region"));
    }
    st.selection.objects.first().copied().ok_or_else(|| bad(cmd, "no selected region"))
}
pub(super) fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.region.get";
    let id = selected_id(s, p, cmd)?;
    let doc = &s.doc()?.doc;
    let r = doc.node(id).and_then(|n| n.extras().nurbs_region.as_deref()).ok_or_else(|| bad(cmd, "not a NURBS region"))?;
    Ok(json!({"id":id.0,"outer":r.outer,"inner":r.inner,"profile":r.profile,"problem":r.problem(doc)}))
}
pub(super) fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "nurbs.region.update";
    let id = selected_id(s, p, cmd)?;
    let doc = &s.doc()?.doc;
    if !doc.is_editable(id) {
        return Err(bad(cmd, "region is locked or hidden"));
    }
    let mut r = doc.node(id).and_then(|n| n.extras().nurbs_region.as_deref()).cloned().ok_or_else(|| bad(cmd, "not a NURBS region"))?;
    let previous = (r.outer, r.inner);
    for (key, slot) in [("outer", &mut r.outer), ("inner", &mut r.inner)] {
        if let Some(v) = p.get(key) {
            *slot = if v.is_null() { None } else { Some(id_param(p, key).ok_or_else(|| bad(cmd, "source must be an id or null"))?) };
        }
    }
    if switch_or(p, "swap", false, cmd)? {
        std::mem::swap(&mut r.outer, &mut r.inner);
    }
    if let Some(profile) = p.get("profile") {
        r.profile = serde_json::from_value(profile.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    }
    // Validate the spline without requiring healthy links: invalid links are a supported state.
    let r: Region = serde_json::from_value(serde_json::to_value(r).map_err(|e| bad(cmd, e.to_string()))?).map_err(|e| bad(cmd, e.to_string()))?;
    let replacements = [r.outer.filter(|v| Some(*v) != previous.0), r.inner.filter(|v| Some(*v) != previous.1)];
    s.edit("Edit NURBS Region", move |doc, _| {
        for source in replacements.into_iter().flatten() {
            if doc.is_editable(source)
                && vectorcraft_doc::nurbs_region::source(doc, source).is_some()
                && let Some(n) = doc.node_mut(source)
            {
                for item in &mut n.appearance.items {
                    *item.paint_mut() = Paint::None;
                }
            }
        }
        doc.node_mut(id).ok_or_else(|| bad(cmd, "region missing"))?.extras_mut().nurbs_region = Some(Box::new(r));
        Ok(())
    })?;
    get(s, &json!({"id":id.0}))
}

#[cfg(test)]
mod nurbs_region_tests {
    use super::*;
    use vectorcraft_doc::nurbs_region::source;
    fn curve(s: &mut Session, x: f64, y: f64, size: f64) -> NodeId {
        NodeId(s.execute("nurbs.create",&json!({"controls":[{"x":x,"y":y},{"x":x+size,"y":y},{"x":x+size,"y":y+size},{"x":x,"y":y+size}],"corners":[0,1,2,3],"periodic":true})).unwrap()["id"].as_u64().unwrap())
    }
    fn setup() -> (Session, NodeId, NodeId, NodeId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width":120,"height":120})).unwrap();
        let outer = curve(&mut s, 10.0, 10.0, 100.0);
        let inner = curve(&mut s, 40.0, 40.0, 40.0);
        s.select(|_, sel| sel.set([inner, outer])).unwrap();
        let region = NodeId(s.execute("nurbs.region.create", &json!({})).unwrap()["id"].as_u64().unwrap());
        (s, outer, inner, region)
    }
    #[test]
    fn nurbs_region_creation_picks_larger_outer_and_is_one_undo() {
        let (mut s, outer, inner, region) = setup();
        let doc = &s.doc().unwrap().doc;
        let node = doc.node(region).unwrap();
        assert_eq!(node.extras().nurbs_region.as_deref(), Some(&Region::new(outer, inner)));
        assert!(node.extras().mask.is_some());
        assert_eq!(node.extras().nurbs_region.as_ref().unwrap().profile.points, vec![[0.0, 1.0], [1.0, 0.0]]);
        assert!(node.appearance.stroke_paint().is_none());
        assert!(doc.node(outer).unwrap().appearance.fill_paint().is_none());
        assert!(source(doc, outer).is_some());
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.node(region).is_none());
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.node(region).unwrap().extras().mask.is_some());
    }
    #[test]
    fn nurbs_region_follows_edits_and_missing_open_sources_recover_with_undo() {
        let (mut s, outer, inner, region) = setup();
        let original = s.doc().unwrap().doc.node(region).unwrap().clone();
        s.edit("Move source", |doc, _| {
            doc.node_mut(outer).unwrap().transform(Affine::translate((7.0, 0.0)), false);
            Ok(())
        })
        .unwrap();
        assert_ne!(s.doc().unwrap().doc.node(region).unwrap().kind, original.kind);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.node(region).unwrap().kind, original.kind);
        s.execute("nurbs.periodic", &json!({"id":inner.0,"periodic":false})).unwrap();
        let n = s.doc().unwrap().doc.node(region).unwrap();
        assert!(n.geometric_bounds().is_none());
        assert!(n.extras().mask.is_none());
        assert!(n.extras().nurbs_region.as_deref().unwrap().problem(&s.doc().unwrap().doc).is_some());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.node(region).unwrap().kind, original.kind);
        s.edit("Delete source", |doc, _| {
            doc.remove(outer)?;
            Ok(())
        })
        .unwrap();
        assert!(s.doc().unwrap().doc.node(region).unwrap().geometric_bounds().is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.node(region).unwrap().kind, original.kind);
    }
    #[test]
    fn nurbs_region_replace_swap_detach_and_invalid_profile_are_atomic() {
        let (mut s, outer, inner, region) = setup();
        let third = curve(&mut s, 45.0, 45.0, 20.0);
        s.execute("nurbs.region.update", &json!({"id":region.0,"inner":third.0})).unwrap();
        assert_eq!(s.execute("nurbs.region.get", &json!({"id":region.0})).unwrap()["inner"], third.0);
        s.execute("nurbs.region.update", &json!({"id":region.0,"swap":true})).unwrap();
        let read = s.execute("nurbs.region.get", &json!({"id":region.0})).unwrap();
        assert_eq!(read["outer"], third.0);
        assert_eq!(read["inner"], outer.0);
        s.execute("nurbs.region.update", &json!({"id":region.0,"outer":null})).unwrap();
        assert!(s.doc().unwrap().doc.node(region).unwrap().geometric_bounds().is_none());
        s.execute("nurbs.region.update", &json!({"id":region.0,"outer":outer.0,"inner":inner.0})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("nurbs.region.update", &json!({"id":region.0,"profile":{"points":[[0,0],[0,1]]}})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("nurbs.region.update", &json!({"id":region.0,"outer":region.0})).unwrap();
        assert!(s.doc().unwrap().doc.node(region).unwrap().geometric_bounds().is_none());
    }
    #[test]
    fn nurbs_region_native_roundtrip_keeps_links_profile_and_mask_pixels() {
        let (mut s, outer, inner, region) = setup();
        s.execute("nurbs.region.update", &json!({"id":region.0,"profile":{"points":[[0,1],[0.5,0.8],[1,0]]}})).unwrap();
        let saved = vectorcraft_format::save_file(&s.doc().unwrap().doc);
        let loaded = vectorcraft_format::load(&saved).unwrap();
        let node = loaded.node(region).unwrap();
        let r = node.extras().nurbs_region.as_deref().unwrap();
        assert_eq!((r.outer, r.inner), (Some(outer), Some(inner)));
        assert_eq!(r.profile.points, vec![[0.0, 1.0], [0.5, 0.8], [1.0, 0.0]]);
        assert_eq!(node, s.doc().unwrap().doc.node(region).unwrap());
        let mut images = 0;
        loaded.visit_images(|_, im| {
            assert!(loaded.image_blob(im).unwrap().sample(0.5, 0.5, 1).is_some());
            images += 1;
        });
        assert_eq!(images, 1);
    }
    #[test]
    fn nurbs_region_preview_is_one_undo_and_escape_restores_mask() {
        let (mut s, _, _, region) = setup();
        let original = s.doc().unwrap().doc.node(region).unwrap().clone();
        s.begin_interaction("Region spline").unwrap();
        for alpha in [0.2, 0.4, 0.7] {
            s.preview("nurbs.region.update", &json!({"id":region.0,"profile":{"points":[[0,alpha],[1,alpha]]}})).unwrap();
        }
        s.cancel_interaction().unwrap();
        assert_eq!(s.doc().unwrap().doc.node(region).unwrap(), &original);
        s.begin_interaction("Region spline").unwrap();
        for alpha in [0.2, 0.7] {
            s.preview("nurbs.region.update", &json!({"id":region.0,"profile":{"points":[[0,alpha],[1,alpha]]}})).unwrap();
        }
        s.commit_interaction().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.node(region).unwrap(), &original);
    }
}

#[cfg(test)]
mod nurbs_region_export_tests {
    use super::*;
    #[test]
    fn nurbs_region_svg_pdf_and_invalid_native_cache() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width":120,"height":120})).unwrap();
        let mut ids = vec![];
        for (x, size) in [(10.0, 100.0), (40.0, 40.0)] {
            ids.push(NodeId(s.execute("nurbs.create",&json!({"controls":[{"x":x,"y":x},{"x":x+size,"y":x},{"x":x+size,"y":x+size},{"x":x,"y":x+size}],"periodic":true,"corners":[0,1,2,3]})).unwrap()["id"].as_u64().unwrap()));
        }
        let id = NodeId(s.execute("nurbs.region.create", &json!({"outer":ids[0],"inner":ids[1]})).unwrap()["id"].as_u64().unwrap());
        let doc = &s.doc().unwrap().doc;
        let svg = vectorcraft_svg::export(doc, &Default::default());
        assert!(svg.contains("<mask") && svg.contains("data:image/png"), "{svg}");
        let pdf = vectorcraft_pdf::export(doc, &Default::default()).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let mut stale = (**doc).clone();
        stale.remove(ids[1]).unwrap();
        assert!(stale.node(id).unwrap().geometric_bounds().is_some());
        let saved = vectorcraft_format::save_file(&stale);
        let loaded = vectorcraft_format::load(&saved).unwrap();
        assert!(loaded.node(id).unwrap().geometric_bounds().is_none());
        assert!(loaded.node(id).unwrap().extras().mask.is_none());
    }
}
