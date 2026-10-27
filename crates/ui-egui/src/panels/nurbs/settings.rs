//! Convenient access to ordinary stroke settings.
use super::*;
use vectorcraft_doc::{AppearanceItem, LineCap, LineJoin};
pub(super) fn show(app: &mut VectorcraftApp, ui: &mut Ui, id: Option<NodeId>) {
    ui.collapsing("Stroke settings", |ui| {
        if let Some(id) = id {
            let stroke = app.session.active().and_then(|s| s.doc.node(id)).and_then(|n| {
                n.appearance.items.iter().find_map(|item| match item {
                    AppearanceItem::Stroke(s) => Some(s.clone()),
                    _ => None,
                })
            });
            let mut cap = stroke.as_ref().map_or(LineCap::Butt, |s| s.cap);
            let mut join = stroke.as_ref().map_or(LineJoin::Miter, |s| s.join);
            let old = (cap, join);
            ui.label("Ends (open curves):");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut cap, LineCap::Butt, "Butt");
                ui.selectable_value(&mut cap, LineCap::Round, "Round");
                ui.selectable_value(&mut cap, LineCap::Square, "Square");
            });
            ui.label("Corners:");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut join, LineJoin::Miter, "Miter");
                ui.selectable_value(&mut join, LineJoin::Round, "Round");
                ui.selectable_value(&mut join, LineJoin::Bevel, "Bevel");
            });
            if (cap, join) != old {
                super::color::commit(app, ui.ctx());
                let cap = match cap {
                    LineCap::Butt => "butt",
                    LineCap::Round => "round",
                    LineCap::Square => "square",
                };
                let join = match join {
                    LineJoin::Miter => "miter",
                    LineJoin::Round => "round",
                    LineJoin::Bevel => "bevel",
                };
                if let Err(e) = app.run("stroke.set", json!({"id":id.0,"cap":cap,"join":join})) {
                    ui.colored_label(ui.visuals().error_fg_color, e.to_string());
                }
            }
            if ui.button("Stroke panel / miter limit...").clicked() {
                super::color::commit(app, ui.ctx());
                app.run("window.panel", json!({"panel":"stroke"})).ok();
            }
        }
    });
}
