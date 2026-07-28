use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke, epaint::CircleShape};
use gradial_core::SliceCount;

const RING_RADIUS_DIP: f32 = 84.0;
const RING_WIDTH_DIP: f32 = 46.0;
const INNER_RADIUS_DIP: f32 = RING_RADIUS_DIP - RING_WIDTH_DIP / 2.0;
const OUTER_RADIUS_DIP: f32 = RING_RADIUS_DIP + RING_WIDTH_DIP / 2.0;
const LABEL_RADIUS_DIP: f32 = 84.0;

/// Narrow rendering boundary used by the interaction layer.
pub(crate) trait OverlayRenderer {
    fn render(&mut self, ui: &mut egui::Ui);
}

#[derive(Debug, Default)]
pub(crate) struct FoundationOverlayRenderer;

impl OverlayRenderer for FoundationOverlayRenderer {
    fn render(&mut self, ui: &mut egui::Ui) {
        let center = ui.max_rect().center();
        let painter = ui.painter();

        painter.add(CircleShape {
            center,
            radius: RING_RADIUS_DIP,
            fill: Color32::TRANSPARENT,
            stroke: Stroke::new(RING_WIDTH_DIP, Color32::from_rgba_unmultiplied(18, 23, 35, 224)),
        });

        painter.circle_stroke(
            center,
            OUTER_RADIUS_DIP,
            Stroke::new(1.5, Color32::from_rgba_unmultiplied(124, 154, 255, 210)),
        );
        painter.circle_stroke(
            center,
            INNER_RADIUS_DIP,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(124, 154, 255, 125)),
        );

        draw_slice_boundaries(painter, center);
        draw_slice_labels(painter, center);

        painter.circle_filled(center, 28.0, Color32::from_rgba_unmultiplied(8, 12, 20, 235));
        painter.text(
            center,
            Align2::CENTER_CENTER,
            "GRADIAL",
            FontId::proportional(11.0),
            Color32::from_rgba_unmultiplied(225, 232, 255, 245),
        );
    }
}

fn draw_slice_boundaries(painter: &egui::Painter, center: Pos2) {
    let half_slice_angle = std::f32::consts::FRAC_PI_8;

    for index in 0..SliceCount::Eight.get() {
        let center_angle = SliceCount::Eight.center_angle(index).expect("valid foundation slice");
        let boundary_angle = center_angle - half_slice_angle;
        let direction = egui::Vec2::new(boundary_angle.sin(), -boundary_angle.cos());

        painter.line_segment(
            [center + direction * INNER_RADIUS_DIP, center + direction * OUTER_RADIUS_DIP],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(124, 154, 255, 88)),
        );
    }
}

fn draw_slice_labels(painter: &egui::Painter, center: Pos2) {
    const LABELS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

    for (index, label) in LABELS.into_iter().enumerate() {
        let direction = SliceCount::Eight.unit_vector(index).expect("valid foundation slice");
        let label_position = center + egui::Vec2::new(direction.x, direction.y) * LABEL_RADIUS_DIP;

        painter.text(
            label_position,
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(12.0),
            Color32::from_rgba_unmultiplied(225, 232, 255, 230),
        );
    }
}
