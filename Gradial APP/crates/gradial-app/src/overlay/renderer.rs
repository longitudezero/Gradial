use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Stroke,
    epaint::{CircleShape, Shape},
};
use gradial_core::SliceCount;

const RING_RADIUS_DIP: f32 = 84.0;
const RING_WIDTH_DIP: f32 = 46.0;
const INNER_RADIUS_DIP: f32 = RING_RADIUS_DIP - RING_WIDTH_DIP / 2.0;
const OUTER_RADIUS_DIP: f32 = RING_RADIUS_DIP + RING_WIDTH_DIP / 2.0;
const LABEL_RADIUS_DIP: f32 = 84.0;
const SELECTED_ARC_STEPS: u8 = 24;
const SELECTED_ARC_INSET_RADIANS: f32 = 0.035;

/// Narrow rendering boundary used by the interaction layer.
pub(crate) trait OverlayRenderer {
    fn render(&mut self, ui: &mut egui::Ui);
}

#[derive(Debug, Default)]
pub(crate) struct FoundationOverlayRenderer {
    selected_slice: Option<usize>,
}

impl FoundationOverlayRenderer {
    pub(crate) fn set_selected_slice(&mut self, selected_slice: Option<usize>) {
        self.selected_slice = selected_slice;
    }
}

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

        if let Some(selected_slice) = self.selected_slice {
            draw_selected_slice(painter, center, selected_slice);
        }

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
            "M4",
            FontId::proportional(11.0),
            Color32::from_rgba_unmultiplied(225, 232, 255, 245),
        );
    }
}

fn draw_selected_slice(painter: &egui::Painter, center: Pos2, selected_slice: usize) {
    let center_angle =
        SliceCount::Eight.center_angle(selected_slice).expect("selected slice is validated");
    let half_slice_angle = std::f32::consts::FRAC_PI_8;
    let start_angle = center_angle - half_slice_angle + SELECTED_ARC_INSET_RADIANS;
    let end_angle = center_angle + half_slice_angle - SELECTED_ARC_INSET_RADIANS;
    let mut points = Vec::with_capacity(usize::from(SELECTED_ARC_STEPS) + 1);

    for step in 0..=SELECTED_ARC_STEPS {
        let progress = f32::from(step) / f32::from(SELECTED_ARC_STEPS);
        let angle = start_angle + (end_angle - start_angle) * progress;
        let direction = egui::Vec2::new(angle.sin(), -angle.cos());
        points.push(center + direction * RING_RADIUS_DIP);
    }

    painter.add(Shape::line(
        points,
        Stroke::new(RING_WIDTH_DIP - 5.0, Color32::from_rgba_unmultiplied(91, 135, 255, 242)),
    ));
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
