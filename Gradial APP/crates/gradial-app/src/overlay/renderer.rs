use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke, Vec2, epaint::CircleShape};
use gradial_core::{ResolvedMenu, SliceCount};
use uuid::Uuid;

use super::animation::{AnimationMode, selection_progress};

const RING_RADIUS_DIP: f32 = 84.0;
const RING_WIDTH_DIP: f32 = 46.0;
const INNER_RADIUS_DIP: f32 = RING_RADIUS_DIP - RING_WIDTH_DIP / 2.0;
const OUTER_RADIUS_DIP: f32 = RING_RADIUS_DIP + RING_WIDTH_DIP / 2.0;
const LABEL_RADIUS_DIP: f32 = 84.0;
const SELECTED_ARC_STEPS: u8 = 24;
const SELECTED_ARC_INSET_RADIANS: f32 = 0.035;
const MAX_SLICE_COUNT: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct OverlayRenderFrame {
    pub(crate) invocation_id: Option<Uuid>,
    pub(crate) menu: Option<Arc<ResolvedMenu>>,
    pub(crate) selected_slice: Option<usize>,
    pub(crate) selection_changed_at: Option<Instant>,
    pub(crate) opacity: f32,
    pub(crate) scale: f32,
    pub(crate) animation_mode: AnimationMode,
    pub(crate) now: Instant,
}

/// Narrow rendering boundary used by the interaction layer.
pub(crate) trait OverlayRenderer {
    /// Returns true while the selection transition needs another frame.
    fn render(&mut self, ui: &mut egui::Ui, frame: OverlayRenderFrame) -> bool;
}

#[derive(Debug)]
pub(crate) struct FoundationOverlayRenderer {
    geometry: RendererGeometryCaches,
    active_invocation: Option<Uuid>,
    selection: SelectionTransition,
}

impl Default for FoundationOverlayRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl FoundationOverlayRenderer {
    pub(crate) fn new() -> Self {
        Self {
            geometry: RendererGeometryCaches::new(),
            active_invocation: None,
            selection: SelectionTransition::default(),
        }
    }

    #[cfg(test)]
    fn cached_geometry_counts(&self, slice_count: SliceCount) -> (usize, usize, usize) {
        let geometry = self.geometry.for_slice_count(slice_count);
        (geometry.boundaries.len(), geometry.label_offsets.len(), geometry.selected_arcs.len())
    }
}

impl OverlayRenderer for FoundationOverlayRenderer {
    fn render(&mut self, ui: &mut egui::Ui, frame: OverlayRenderFrame) -> bool {
        if self.active_invocation != frame.invocation_id {
            self.active_invocation = frame.invocation_id;
            self.selection.reset(None, frame.selection_changed_at.unwrap_or(frame.now));
        }
        let (selection_levels, selection_animating) = self.selection.sample(
            frame.selected_slice,
            frame.selection_changed_at,
            frame.now,
            frame.animation_mode,
        );
        let Some(menu) = frame.menu.as_deref() else {
            return selection_animating;
        };
        let geometry = self.geometry.for_slice_count(menu.slice_count());

        let center = ui.max_rect().center();
        let painter = ui.painter();
        let opacity = frame.opacity.clamp(0.0, 1.0);
        let scale = frame.scale.clamp(0.0, 1.0);

        painter.add(CircleShape {
            center,
            radius: RING_RADIUS_DIP * scale,
            fill: Color32::TRANSPARENT,
            stroke: Stroke::new(
                RING_WIDTH_DIP * scale,
                faded(Color32::from_rgba_unmultiplied(18, 23, 35, 224), opacity),
            ),
        });

        draw_selected_slices(painter, center, geometry, selection_levels, opacity, scale);

        painter.circle_stroke(
            center,
            OUTER_RADIUS_DIP * scale,
            Stroke::new(
                1.5 * scale,
                faded(Color32::from_rgba_unmultiplied(124, 154, 255, 210), opacity),
            ),
        );
        painter.circle_stroke(
            center,
            INNER_RADIUS_DIP * scale,
            Stroke::new(scale, faded(Color32::from_rgba_unmultiplied(124, 154, 255, 125), opacity)),
        );

        draw_slice_boundaries(painter, center, geometry, opacity, scale);
        draw_slice_labels(painter, center, geometry, menu.labels(), opacity, scale);

        painter.circle_filled(
            center,
            28.0 * scale,
            faded(Color32::from_rgba_unmultiplied(8, 12, 20, 235), opacity),
        );
        painter.text(
            center,
            Align2::CENTER_CENTER,
            "M4",
            FontId::proportional(11.0 * scale),
            faded(Color32::from_rgba_unmultiplied(225, 232, 255, 245), opacity),
        );

        selection_animating
    }
}

#[derive(Debug)]
struct RendererGeometryCaches {
    four: RendererGeometryCache,
    eight: RendererGeometryCache,
}

impl RendererGeometryCaches {
    fn new() -> Self {
        Self {
            four: RendererGeometryCache::new(SliceCount::Four),
            eight: RendererGeometryCache::new(SliceCount::Eight),
        }
    }

    fn for_slice_count(&self, slice_count: SliceCount) -> &RendererGeometryCache {
        match slice_count {
            SliceCount::Four => &self.four,
            SliceCount::Eight => &self.eight,
        }
    }
}

#[derive(Debug)]
struct RendererGeometryCache {
    boundaries: Vec<[Vec2; 2]>,
    label_offsets: Vec<Vec2>,
    selected_arcs: Vec<Vec<[Vec2; 2]>>,
}

impl RendererGeometryCache {
    fn new(slice_count: SliceCount) -> Self {
        let half_slice_angle = half_slice_angle(slice_count);
        let mut boundaries = Vec::with_capacity(slice_count.get());
        let mut label_offsets = Vec::with_capacity(slice_count.get());
        let mut selected_arcs = Vec::with_capacity(slice_count.get());

        for index in 0..slice_count.get() {
            let center_angle = slice_count.center_angle(index).expect("valid cached slice");
            let boundary_angle = center_angle - half_slice_angle;
            let boundary_direction = Vec2::new(boundary_angle.sin(), -boundary_angle.cos());
            boundaries.push([
                boundary_direction * INNER_RADIUS_DIP,
                boundary_direction * OUTER_RADIUS_DIP,
            ]);

            let label_direction = slice_count.unit_vector(index).expect("valid cached slice");
            label_offsets.push(Vec2::new(label_direction.x, label_direction.y) * LABEL_RADIUS_DIP);

            selected_arcs.push(selected_arc_segments(center_angle, half_slice_angle));
        }

        Self { boundaries, label_offsets, selected_arcs }
    }
}

#[derive(Debug)]
struct SelectionTransition {
    target: Option<usize>,
    from: [f32; MAX_SLICE_COUNT],
    displayed: [f32; MAX_SLICE_COUNT],
    started_at: Instant,
}

impl Default for SelectionTransition {
    fn default() -> Self {
        Self {
            target: None,
            from: [0.0; MAX_SLICE_COUNT],
            displayed: [0.0; MAX_SLICE_COUNT],
            started_at: Instant::now(),
        }
    }
}

impl SelectionTransition {
    fn reset(&mut self, target: Option<usize>, now: Instant) {
        self.target = target;
        self.from = target_levels(target);
        self.displayed = self.from;
        self.started_at = now;
    }

    fn sample(
        &mut self,
        target: Option<usize>,
        changed_at: Option<Instant>,
        now: Instant,
        mode: AnimationMode,
    ) -> ([f32; MAX_SLICE_COUNT], bool) {
        if mode.is_reduced() {
            self.reset(target, now);
            return (self.displayed, false);
        }

        if target != self.target {
            let transition_time = changed_at.unwrap_or(now);
            self.advance(transition_time);
            self.from = self.displayed;
            self.target = target;
            self.started_at = transition_time;
        }

        let complete = self.advance(now);
        (self.displayed, !complete)
    }

    fn advance(&mut self, now: Instant) -> bool {
        let progress = selection_progress(now.saturating_duration_since(self.started_at));
        let targets = target_levels(self.target);
        for (index, target) in targets.iter().enumerate() {
            self.displayed[index] = self.from[index] + (*target - self.from[index]) * progress;
        }
        progress >= 1.0
    }
}

fn target_levels(selected_slice: Option<usize>) -> [f32; MAX_SLICE_COUNT] {
    let mut levels = [0.0; MAX_SLICE_COUNT];
    if let Some(selected_slice) = selected_slice
        && let Some(level) = levels.get_mut(selected_slice)
    {
        *level = 1.0;
    }
    levels
}

fn selected_arc_segments(center_angle: f32, half_slice_angle: f32) -> Vec<[Vec2; 2]> {
    let start_angle = center_angle - half_slice_angle + SELECTED_ARC_INSET_RADIANS;
    let end_angle = center_angle + half_slice_angle - SELECTED_ARC_INSET_RADIANS;
    let mut segments = Vec::with_capacity(usize::from(SELECTED_ARC_STEPS));
    let mut previous = arc_offset(start_angle);

    for step in 1..=SELECTED_ARC_STEPS {
        let progress = f32::from(step) / f32::from(SELECTED_ARC_STEPS);
        let current = arc_offset(start_angle + (end_angle - start_angle) * progress);
        segments.push([previous, current]);
        previous = current;
    }
    segments
}

fn arc_offset(angle: f32) -> Vec2 {
    Vec2::new(angle.sin(), -angle.cos()) * RING_RADIUS_DIP
}

fn draw_selected_slices(
    painter: &egui::Painter,
    center: Pos2,
    geometry: &RendererGeometryCache,
    levels: [f32; MAX_SLICE_COUNT],
    opacity: f32,
    scale: f32,
) {
    for (index, segments) in geometry.selected_arcs.iter().enumerate() {
        let intensity = levels[index];
        if intensity <= f32::EPSILON {
            continue;
        }
        let stroke = Stroke::new(
            (RING_WIDTH_DIP - 5.0) * scale,
            faded(Color32::from_rgba_unmultiplied(91, 135, 255, 242), opacity * intensity),
        );
        for segment in segments {
            painter
                .line_segment([center + segment[0] * scale, center + segment[1] * scale], stroke);
        }
    }
}

fn draw_slice_boundaries(
    painter: &egui::Painter,
    center: Pos2,
    geometry: &RendererGeometryCache,
    opacity: f32,
    scale: f32,
) {
    let stroke =
        Stroke::new(scale, faded(Color32::from_rgba_unmultiplied(124, 154, 255, 88), opacity));
    for segment in &geometry.boundaries {
        painter.line_segment([center + segment[0] * scale, center + segment[1] * scale], stroke);
    }
}

fn draw_slice_labels(
    painter: &egui::Painter,
    center: Pos2,
    geometry: &RendererGeometryCache,
    labels: &[String],
    opacity: f32,
    scale: f32,
) {
    let color = faded(Color32::from_rgba_unmultiplied(225, 232, 255, 230), opacity);
    for (offset, label) in menu_label_layout(geometry, labels) {
        painter.text(
            center + offset * scale,
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(12.0 * scale),
            color,
        );
    }
}

fn menu_label_layout<'a>(
    geometry: &'a RendererGeometryCache,
    labels: &'a [String],
) -> impl Iterator<Item = (Vec2, &'a str)> + 'a {
    geometry.label_offsets.iter().copied().zip(labels.iter().map(String::as_str))
}

fn faded(color: Color32, opacity: f32) -> Color32 {
    color.gamma_multiply(opacity.clamp(0.0, 1.0))
}

const fn half_slice_angle(slice_count: SliceCount) -> f32 {
    match slice_count {
        SliceCount::Four => std::f32::consts::FRAC_PI_4,
        SliceCount::Eight => std::f32::consts::FRAC_PI_8,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use std::sync::Arc;

    use gradial_core::{ResolvedMenu, SliceCount};

    use super::{
        FoundationOverlayRenderer, SelectionTransition, half_slice_angle, menu_label_layout,
    };
    use crate::overlay::AnimationMode;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn four_slice_renderer_uses_cardinal_quadrants() {
        assert!(
            (half_slice_angle(SliceCount::Four) - std::f32::consts::FRAC_PI_4).abs() < f32::EPSILON
        );
    }

    #[test]
    fn default_renderer_prewarms_both_supported_geometries() {
        let _renderer = FoundationOverlayRenderer::default();
    }

    #[test]
    fn renderer_caches_every_geometry_once_per_display() {
        let renderer = FoundationOverlayRenderer::new();
        assert_eq!(renderer.cached_geometry_counts(SliceCount::Four), (4, 4, 4));
        assert_eq!(renderer.cached_geometry_counts(SliceCount::Eight), (8, 8, 8));
        assert!(renderer.geometry.four.selected_arcs.iter().all(|arc| arc.len() == 24));
        assert!(renderer.geometry.eight.selected_arcs.iter().all(|arc| arc.len() == 24));
    }

    #[test]
    fn renderer_layout_uses_the_resolved_profile_labels() {
        let renderer = FoundationOverlayRenderer::new();
        let menu = ResolvedMenu::new(
            SliceCount::Four,
            Arc::from([
                "Build".to_owned(),
                "Search".to_owned(),
                "Tests".to_owned(),
                "Terminal".to_owned(),
            ]),
        )
        .expect("complete custom menu");
        let labels: Vec<_> =
            menu_label_layout(renderer.geometry.for_slice_count(menu.slice_count()), menu.labels())
                .map(|(_, label)| label)
                .collect();

        assert_eq!(labels, ["Build", "Search", "Tests", "Terminal"]);
    }

    #[test]
    fn selection_color_reaches_target_in_thirty_five_milliseconds() {
        let start = Instant::now();
        let mut transition = SelectionTransition::default();
        transition.reset(None, start);

        let (first, animating) =
            transition.sample(Some(2), Some(start), start, AnimationMode::Standard);
        assert_close(first[2], 0.0);
        assert!(animating);

        let (middle, animating) = transition.sample(
            Some(2),
            Some(start),
            start + Duration::from_millis(17),
            AnimationMode::Standard,
        );
        assert!(middle[2] > 0.48 && middle[2] < 0.49);
        assert!(animating);

        let (last, animating) = transition.sample(
            Some(2),
            Some(start),
            start + Duration::from_millis(35),
            AnimationMode::Standard,
        );
        assert_close(last[2], 1.0);
        assert!(!animating);
    }

    #[test]
    fn reduced_motion_applies_selection_without_interpolation() {
        let now = Instant::now();
        let mut transition = SelectionTransition::default();
        let (levels, animating) =
            transition.sample(Some(1), Some(now), now, AnimationMode::ReducedMotion);
        assert_close(levels[1], 1.0);
        assert!(!animating);
    }

    #[test]
    fn selection_retarget_starts_from_the_current_color() {
        let start = Instant::now();
        let mut transition = SelectionTransition::default();
        transition.reset(None, start);
        let _first = transition.sample(Some(1), Some(start), start, AnimationMode::Standard);
        let halfway = start + Duration::from_millis(17);
        let _middle = transition.sample(Some(1), Some(start), halfway, AnimationMode::Standard);
        let (retargeted, _) =
            transition.sample(Some(2), Some(halfway), halfway, AnimationMode::Standard);
        assert!(retargeted[1] > 0.48 && retargeted[1] < 0.49);
        assert_close(retargeted[2], 0.0);
    }
}
