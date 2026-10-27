use super::*;
use proptest::prelude::*;

fn cp(x: f64, y: f64) -> ControlPoint {
    ControlPoint::new(Point::new(x, y), 1.0)
}
fn example() -> NurbsCurve {
    NurbsCurve::from_controls(vec![cp(0.0, 0.0), cp(1.0, 4.0), cp(3.0, -2.0), cp(5.0, 3.0), cp(8.0, 0.0)]).unwrap()
}
fn same(a: &NurbsCurve, b: &NurbsCurve) {
    for i in 0..=200 {
        let u = i as f64 / 200.0;
        assert!(a.evaluate(u).unwrap().distance(b.evaluate(u).unwrap()) < 1e-10, "u={u}");
    }
}

#[test]
fn exact_rational_quarter_circle() {
    let c = NurbsCurve::from_controls(vec![cp(1.0, 0.0), ControlPoint::new(Point::new(1.0, 1.0), 0.5_f64.sqrt()), cp(0.0, 1.0)]).unwrap();
    for i in 0..=100 {
        let p = c.evaluate(i as f64 / 100.0).unwrap();
        assert!((p.x * p.x + p.y * p.y - 1.0).abs() < 1e-14);
    }
    same(&c, &c.insert_knot(0.37).unwrap());
}

#[test]
fn automatic_degree_after_topology_edits() {
    let line = NurbsCurve::from_controls(vec![cp(0.0, 0.0), cp(2.0, 2.0)]).unwrap();
    assert_eq!(line.degree(), 1);
    assert_eq!(line.evaluate(0.5).unwrap(), Point::new(1.0, 1.0));
    let quadratic = line.append_control(cp(4.0, 0.0)).unwrap();
    let cubic = quadratic.append_control(cp(6.0, 3.0)).unwrap();
    assert_eq!(quadratic.degree(), 2);
    assert_eq!(cubic.degree(), 3);
    assert_eq!(cubic.remove_control(2).unwrap().degree(), 2);
    assert_eq!(quadratic.remove_control(1).unwrap().degree(), 1);
    assert!(line.remove_control(0).is_err());
}

#[test]
fn repeated_knot_insertion_and_split_preserve_parameterized_curve() {
    let c = example();
    let mut refined = c.clone();
    for _ in 0..3 {
        refined = refined.insert_knot(0.37).unwrap();
        same(&c, &refined);
    }
    assert_eq!(refined.insert_knot(0.37), Err(NurbsError::Multiplicity));
    for u in [0.01, 0.37, 0.5, 0.99] {
        let (left, right) = c.split(u).unwrap();
        assert_eq!(left.controls().last(), right.controls().first());
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            assert!(left.evaluate(t).unwrap().distance(c.evaluate(u * t).unwrap()) < 1e-10);
            assert!(right.evaluate(t).unwrap().distance(c.evaluate(u + (1.0 - u) * t).unwrap()) < 1e-10);
        }
        let joined = left.join(&right).unwrap();
        assert_eq!(joined.controls().len(), left.controls().len() + right.controls().len() - 1);
        assert_eq!(joined.evaluate(0.0).unwrap(), c.evaluate(0.0).unwrap());
        assert_eq!(joined.evaluate(1.0).unwrap(), c.evaluate(1.0).unwrap());
    }
}

#[test]
fn invalid_input_is_rejected_without_mutation() {
    let c = example();
    let old = c.clone();
    for u in [f64::NAN, f64::INFINITY, -1.0, 2.0] {
        assert!(c.evaluate(u).is_err());
    }
    for u in [0.0, 1.0, f64::NAN] {
        assert!(c.split(u).is_err());
        assert!(c.insert_knot(u).is_err());
    }
    for weight in [0.0, -1.0, f64::INFINITY, f64::NAN, 1e-200] {
        assert!(c.set_control(0, ControlPoint::new(Point::ZERO, weight)).is_err());
    }
    assert!(c.set_control(usize::MAX, cp(1.0, 1.0)).is_err());
    assert!(c.remove_control(usize::MAX).is_err());
    assert!(NurbsCurve::new(usize::MAX, vec![], vec![]).is_err());
    assert!(NurbsCurve::new(1, vec![cp(0.0, 0.0), cp(1.0, 1.0)], vec![0.0, 0.0, f64::NAN, 1.0]).is_err());
    assert!(NurbsCurve::from_controls(vec![cp(0.0, 0.0); MAX_CONTROLS + 1]).is_err());
    assert_eq!(c, old);
}

#[test]
fn common_weight_scaling_does_not_change_geometry() {
    let c = example();
    for weight in [1e-90, 1e90] {
        let controls = c.controls.iter().map(|p| ControlPoint::new(p.position, weight)).collect();
        let scaled = NurbsCurve::new(c.degree, controls, c.knots.clone()).unwrap();
        same(&c, &scaled);
        same(&scaled, &scaled.insert_knot(0.23).unwrap());
    }
}

proptest! {
    #[test]
    fn rational_refinement_preserves_curve(points in prop::collection::vec((-1000.0..1000.0, -1000.0..1000.0, 0.1..10.0), 4..20), u in 0.001..0.999) {
        let c = NurbsCurve::from_controls(points.into_iter().map(|(x,y,w)| ControlPoint::new(Point::new(x,y), w)).collect()).unwrap();
        let refined = c.insert_knot(u).unwrap();
        same(&c, &refined);
        let (left, right) = c.split(u).unwrap();
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            prop_assert!(left.evaluate(t).unwrap().distance(c.evaluate(t*u).unwrap()) < 1e-9);
            prop_assert!(right.evaluate(t).unwrap().distance(c.evaluate(u+(1.0-u)*t).unwrap()) < 1e-9);
        }
    }
}

#[test]
fn degree_elevation_and_editor_insertion_preserve_rational_shape() {
    for count in 2..=7 {
        for degree in 1..=2_usize.min(count - 1) {
            let controls = (0..count).map(|i| ControlPoint::new(Point::new(i as f64, (i * i % 5) as f64), 0.3 + i as f64)).collect();
            let mut knots = vec![0.0; degree + 1];
            for i in 1..count - degree {
                knots.push(i as f64 / (count - degree) as f64);
            }
            knots.extend(std::iter::repeat_n(1.0, degree + 1));
            let c = NurbsCurve::new(degree, controls, knots).unwrap();
            let cubic = c.to_cubic().unwrap();
            same(&c, &cubic);
            assert_eq!(cubic.degree(), 3);
            let inserted = c.insert_vertex(0.371).unwrap();
            same(&c, &inserted);
            assert_eq!(inserted.degree(), 3.min(inserted.controls().len() - 1));
            assert!(inserted.knots().contains(&0.371));
        }
    }
}

#[test]
fn serialization_validates_original_geometry() {
    let c = example();
    let wire = serde_json::to_value(&c).unwrap();
    assert_eq!(c, serde_json::from_value::<NurbsCurve>(wire.clone()).unwrap());
    let mut invalid = wire;
    invalid["degree"] = serde_json::json!(7);
    assert!(serde_json::from_value::<NurbsCurve>(invalid).is_err());
}

#[test]
fn display_outline_is_exact_for_polynomial_and_bounded_for_rational() {
    use kurbo::ParamCurveNearest;
    let c = example();
    let outline = c.to_bezpath(0.01).unwrap();
    for i in 0..=200 {
        let point = c.evaluate(i as f64 / 200.0).unwrap();
        let distance = outline.segments().map(|segment| segment.nearest(point, 1e-8).distance_sq).fold(f64::INFINITY, f64::min).sqrt();
        assert!(distance < 1e-5);
    }
    let circle = NurbsCurve::from_controls(vec![cp(1.0, 0.0), ControlPoint::new(Point::new(1.0, 1.0), 0.5_f64.sqrt()), cp(0.0, 1.0)]).unwrap();
    let outline = circle.to_bezpath(0.001).unwrap();
    for i in 0..=200 {
        let point = circle.evaluate(i as f64 / 200.0).unwrap();
        let distance = outline.segments().map(|segment| segment.nearest(point, 1e-8).distance_sq).fold(f64::INFINITY, f64::min).sqrt();
        assert!(distance <= 0.001);
    }
    assert!(c.to_bezpath(f64::NAN).is_err());
}
