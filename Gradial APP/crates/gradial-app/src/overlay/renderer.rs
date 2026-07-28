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
    slice_count: SliceCount,
    selected_slice: Option<usize>,
}

impl FoundationOverlayRenderer {
    pub(crate) const fn new(slice_count: SliceCount) -> Self {
        Self { slice_count, selected_slice: None }
    }

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
            draw_selected_slice(painter, center, self.slice_count, selected_slice);
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

        draw_slice_boundaries(painter, center, self.slice_count);
        draw_slice_labels(painter, center, self.slice_count);

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

fn draw_selected_slice(
    painter: &egui::Painter,
    center: Pos2,
    slice_count: SliceCount,
    selected_slice: usize,
) {
    let center_angle =
        slice_count.center_angle(selected_slice).expect("selected slice is validated");
    let half_slice_angle = half_slice_angle(slice_count);
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

fn draw_slice_boundaries(painter: &egui::Painter, center: Pos2, slice_count: SliceCount) {
    let half_slice_angle = half_slice_angle(slice_count);

    for index in 0..slice_count.get() {
        let center_angle = slice_count.center_angle(index).expect("valid foundation slice");
        let boundary_angle = center_angle - half_slice_angle;
        let direction = egui::Vec2::new(boundary_angle.sin(), -boundary_angle.cos());

        painter.line_segment(
            [center + direction * INNER_RADIUS_DIP, center + direction * OUTER_RADIUS_DIP],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(124, 154, 255, 88)),
        );
    }
}

fn draw_slice_labels(painter: &egui::Painter, center: Pos2, slice_count: SliceCount) {
    for index in 0..slice_count.get() {
        let label = slice_label(slice_count, index).expect("valid foundation slice");
        let direction = slice_count.unit_vector(index).expect("valid foundation slice");
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

const fn half_slice_angle(slice_count: SliceCount) -> f32 {
    match slice_count {
        SliceCount::Four => std::f32::consts::FRAC_PI_4,
        SliceCount::Eight => std::f32::consts::FRAC_PI_8,
    }
}

fn slice_label(slice_count: SliceCount, index: usize) -> Option<&'static str> {
    match slice_count {
        SliceCount::Four => ["N", "E", "S", "W"].get(index).copied(),
        SliceCount::Eight => ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].get(index).copied(),
    }
}

#[cfg(test)]
mod tests {
    use gradial_core::SliceCount;

    use super::{FoundationOverlayRenderer, half_slice_angle, slice_label};

    #[test]
    fn four_slice_renderer_uses_cardinal_labels_and_quadrants() {
        let labels: Vec<_> =
            (0..SliceCount::Four.get()).map(|index| slice_label(SliceCount::Four, index)).collect();
        assert_eq!(labels, [Some("N"), Some("E"), Some("S"), Some("W")]);
        assert!(
            (half_slice_angle(SliceCount::Four) - std::f32::consts::FRAC_PI_4).abs() < f32::EPSILON
        );
    }

    #[test]
    fn renderer_rejects_labels_outside_the_selected_geometry() {
        assert_eq!(slice_label(SliceCount::Four, 4), None);
        assert_eq!(slice_label(SliceCount::Eight, 8), None);
    }

    #[test]
    fn default_renderer_preserves_the_eight_direction_contract() {
        let renderer = FoundationOverlayRenderer::default();
        let labels: Vec<_> = (0..SliceCount::Eight.get())
            .map(|index| slice_label(renderer.slice_count, index))
            .collect();
        assert_eq!(
            labels,
            [
                Some("N"),
                Some("NE"),
                Some("E"),
                Some("SE"),
                Some("S"),
                Some("SW"),
                Some("W"),
                Some("NW"),
            ]
        );
    }
}
