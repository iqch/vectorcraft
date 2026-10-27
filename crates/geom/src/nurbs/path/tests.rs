use super::*;
fn controls() -> Vec<ControlPoint> {
    [(0.0, 0.0), (80.0, 150.0), (150.0, -40.0), (240.0, 110.0), (330.0, 0.0)]
        .into_iter()
        .enumerate()
        .map(|(i, (x, y))| ControlPoint::new(Point::new(x, y), 0.7 + i as f64 * 0.2))
        .collect()
}
fn same(a: &NurbsCurve, b: &NurbsCurve) {
    for i in 0..=200 {
        let u = i as f64 / 200.0;
        assert!(a.evaluate(u).unwrap().distance(b.evaluate(u).unwrap()) < 1e-8, "u={u}");
    }
}
#[test]
fn periodic_seam_refinement_and_clamping() {
    let c = NurbsCurve::periodic_from_controls(controls()).unwrap();
    assert_eq!(c.evaluate(0.0).unwrap(), c.evaluate(1.0).unwrap());
    let h = 1e-6;
    let left = (c.evaluate(1.0).unwrap() - c.evaluate(1.0 - h).unwrap()) / h;
    let right = (c.evaluate(h).unwrap() - c.evaluate(0.0).unwrap()) / h;
    assert!((left - right).hypot() < 0.02);
    same(&c, &c.as_clamped().unwrap());
    for u in [0.0, 0.001, 0.2, 0.371, 0.999, 1.0] {
        same(&c, &c.insert_knot(u).unwrap());
    }
    let mut refined = c.clone();
    for _ in 0..3 {
        refined = refined.insert_knot(0.37).unwrap();
        same(&c, &refined);
    }
    assert!(refined.insert_knot(0.37).is_err());
    let wire = serde_json::to_value(&refined).unwrap();
    assert_eq!(refined, serde_json::from_value::<NurbsCurve>(wire).unwrap());
}
#[test]
fn periodic_automatic_elevation_preserves_small_curves() {
    for n in 2..=5 {
        let controls = controls().into_iter().take(n).collect();
        let c = NurbsCurve::periodic_from_controls(controls).unwrap();
        let cubic = c.to_cubic().unwrap();
        assert!(cubic.is_periodic());
        assert_eq!(cubic.degree(), 3);
        same(&c, &cubic);
        let inserted = c.insert_vertex(0.371).unwrap();
        same(&c, &inserted);
        assert!(inserted.is_periodic());
    }
}
#[test]
fn shared_corner_edits_both_sides_and_removal_merges() {
    let path = NurbsPath::from_controls(controls()).unwrap();
    let corner = path.toggle_corner(2).unwrap();
    assert_eq!(corner.controls(), path.controls());
    assert_eq!(corner.pieces().len(), 2);
    assert_eq!(corner.pieces().first().unwrap().degree(), 2);
    assert_eq!(corner.pieces().last().unwrap().degree(), 2);
    assert_eq!(corner.evaluate(0.5).unwrap(), path.controls().get(2).unwrap().position);
    let point = ControlPoint::new(Point::new(100.0, 200.0), corner.controls()[2].weight);
    let moved = corner.set_control(2, point).unwrap();
    assert_eq!(moved.pieces().first().unwrap().controls().last(), Some(&point));
    assert_eq!(moved.pieces().last().unwrap().controls().first(), Some(&point));
    let merged = moved.toggle_corner(2).unwrap();
    assert!(merged.corners().is_empty());
    assert_eq!(merged.controls(), moved.controls());
    assert_eq!(merged.degree(), 3);
}
#[test]
fn periodic_corners_wrap_and_survive_native_geometry_roundtrip() {
    let open = NurbsPath::from_controls(controls()).unwrap();
    let smooth = open.with_periodic(true).unwrap();
    assert!(smooth.corners().is_empty());
    let corner = smooth.toggle_corner(3).unwrap();
    assert_eq!(corner.corners(), &[3]);
    assert_eq!(corner.controls(), smooth.controls());
    assert!(corner.evaluate((1.0 - corner.parameter_start).rem_euclid(1.0)).unwrap().distance(open.controls()[3].position) < 1e-8);
    let point = ControlPoint::new(Point::new(-50.0, 20.0), corner.controls()[3].weight);
    let moved = corner.set_control(3, point).unwrap();
    assert_eq!(moved.pieces().first().unwrap().controls().first(), Some(&point));
    assert_eq!(moved.pieces().last().unwrap().controls().last(), Some(&point));
    let two = moved.toggle_corner(2).unwrap();
    assert_eq!(two.corners().len(), 2);
    let removed = two.toggle_corner(3).unwrap();
    assert_eq!(removed.corners().len(), 1);
    assert_eq!(removed.controls().len(), open.controls().len());
    let smooth = removed.toggle_corner(2).unwrap();
    assert!(smooth.corners().is_empty());
    assert!(smooth.is_periodic());
    let wire = serde_json::to_value(&two).unwrap();
    assert_eq!(two, serde_json::from_value::<NurbsPath>(wire).unwrap());
    let legacy = NurbsCurve::from_controls(controls()).unwrap();
    let wire = serde_json::to_value(&legacy).unwrap();
    let loaded: NurbsPath = serde_json::from_value(wire).unwrap();
    assert_eq!(loaded.controls(), legacy.controls());
}
#[test]
fn periodic_and_corner_outlines_match_source() {
    use kurbo::ParamCurveNearest;
    for path in [
        NurbsPath::from_controls(controls()).unwrap().with_periodic(true).unwrap(),
        NurbsPath::from_controls(controls()).unwrap().toggle_corner(2).unwrap().with_periodic(true).unwrap(),
    ] {
        let bp = path.to_bezpath(0.01).unwrap();
        assert!(matches!(bp.elements().last(), Some(PathEl::ClosePath)));
        for i in 0..=100 {
            let p = path.evaluate(i as f64 / 100.0).unwrap();
            let distance = bp.segments().map(|s| s.nearest(p, 1e-8).distance_sq).fold(f64::INFINITY, f64::min).sqrt();
            assert!(distance <= 0.011, "distance={distance}");
        }
    }
}

#[test]
fn periodic_uniform_basis_matches_independent_cubic_formula() {
    let points = [Point::new(0.0, 0.0), Point::new(0.0, 120.0), Point::new(120.0, 120.0), Point::new(120.0, 0.0)];
    let curve = NurbsCurve::periodic_from_controls(points.iter().map(|p| ControlPoint::new(*p, 1.0)).collect()).unwrap();
    for i in 0..4 {
        let a = points[i];
        let b = points[(i + 1) % 4];
        let c = points[(i + 2) % 4];
        let expected = Point::new((a.x + 4.0 * b.x + c.x) / 6.0, (a.y + 4.0 * b.y + c.y) / 6.0);
        assert!(curve.evaluate(i as f64 / 4.0).unwrap().distance(expected) < 1e-10);
    }
}
#[test]
fn corner_refinement_and_deletion_keep_topology() {
    let path = NurbsPath::from_controls(controls()).unwrap().toggle_corner(2).unwrap();
    let refined = path.insert_vertex(0.213).unwrap();
    assert_eq!(refined.pieces().len(), 2);
    for i in 0..=100 {
        let u = i as f64 / 100.0;
        assert!(path.evaluate(u).unwrap().distance(refined.evaluate(u).unwrap()) < 1e-8);
    }
    assert_eq!(refined.pieces().last(), path.pieces().last());
    let removed = path.remove_control(1).unwrap();
    assert_eq!(removed.controls().len(), path.controls().len() - 1);
    assert!(removed.is_corner(1));
    assert_eq!(removed.pieces().last(), path.pieces().last());
    assert_eq!(path.remove_control(2).unwrap().corners().len(), 0);
    let periodic = path.with_periodic(true).unwrap();
    assert!(periodic.append_control(ControlPoint::new(Point::new(500.0, 10.0), 1.0)).unwrap().is_periodic());
    assert!(NurbsPath::with_topology(controls(), vec![usize::MAX], true).is_err());
}

use proptest::prelude::*;
proptest! {
    #[test]
    fn random_periodic_refinement_and_seam(points in prop::collection::vec((-100.0..100.0,-100.0..100.0,0.1..5.0),4..12),u in 0.0001..0.9999){
        let c=NurbsCurve::periodic_from_controls(points.into_iter().map(|(x,y,w)|ControlPoint::new(Point::new(x,y),w)).collect()).unwrap();
        let refined=c.insert_knot(u).unwrap().insert_knot(0.0).unwrap();
        same(&c,&refined);same(&c,&refined.as_clamped().unwrap());
        for t in [1e-9,1e-6,u*0.5,u,(1.0+u)*0.5,1.0-1e-6,1.0-1e-9]{
            prop_assert!(c.evaluate(t).unwrap().distance(refined.evaluate(t).unwrap())<1e-8);
        }
        prop_assert_eq!(refined.evaluate(0.0).unwrap(),refined.evaluate(1.0).unwrap());
        let data=serde_json::to_value(&refined).unwrap();
        prop_assert_eq!(&refined,&serde_json::from_value::<NurbsCurve>(data).unwrap());
    }
}

#[test]
fn periodic_corner_edits_preserve_public_control_color_order_and_serialized_seam() {
    let controls: Vec<_> = controls()
        .into_iter()
        .enumerate()
        .map(|(i, mut p)| {
            p.color = [i as f64 / 5.0, 0.2, 0.8, 1.0];
            p
        })
        .collect();
    let original = NurbsPath::with_topology(controls.clone(), vec![], true).unwrap();
    let mut curve = original.clone();
    for index in [3, 1, 4, 0, 2, 3, 4, 0, 2, 1] {
        curve = curve.toggle_corner(index).unwrap();
        assert_eq!(curve.controls(), controls, "control/color order changed at {index}");
        let saved = serde_json::to_value(&curve).unwrap();
        let reopened: NurbsPath = serde_json::from_value(saved).unwrap();
        assert_eq!(reopened, curve);
        for u in [0.0, 0.17, 0.6, 1.0] {
            assert!(curve.evaluate(u).unwrap().distance(reopened.evaluate(u).unwrap()) < 1e-10);
        }
        if !curve.corners().is_empty() {
            let refined = curve.insert_vertex(0.231).unwrap();
            for j in 0..=50 {
                let u = j as f64 / 50.0;
                assert!(curve.evaluate(u).unwrap().distance(refined.evaluate(u).unwrap()) < 1e-7);
            }
        }
    }
    assert!(curve.corners().is_empty());
    assert_eq!(curve.controls(), original.controls());
}
#[test]
fn corner_weight_is_locked_but_position_and_color_remain_editable() {
    let curve = NurbsPath::with_topology(controls(), vec![2], true).unwrap();
    let mut p = curve.controls()[2];
    p.weight *= 2.0;
    assert!(curve.set_control(2, p).is_err());
    p = curve.controls()[2];
    p.position.x += 20.0;
    p.color = [1.0, 0.0, 0.0, 0.5];
    let edited = curve.set_control(2, p).unwrap();
    assert_eq!(edited.controls()[2], p);
    let mut smooth = curve.toggle_corner(2).unwrap();
    p.weight *= 2.0;
    smooth = smooth.set_control(2, p).unwrap();
    assert_eq!(smooth.controls()[2].weight, p.weight);
}
