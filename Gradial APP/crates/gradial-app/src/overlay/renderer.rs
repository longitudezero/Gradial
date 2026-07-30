use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke, Vec2, epaint::CircleShape};
use gradial_core::{
    AccentColor, ActivationBindingId, Appearance, LabelTheme, ResolvedMenu, SliceCount,
};
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
    pub(crate) activation_button: ActivationBindingId,
    pub(crate) appearance: Appearance,
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
        let scale = frame.scale.clamp(0.0, 1.0) * frame.appearance.size_preset.visual_scale();
        let theme = theme_colors(frame.appearance.label_theme);
        let accent = frame.appearance.accent_color;

        painter.add(CircleShape {
            center,
            radius: RING_RADIUS_DIP * scale,
            fill: Color32::TRANSPARENT,
            stroke: Stroke::new(RING_WIDTH_DIP * scale, faded(theme.ring, opacity)),
        });

        draw_selected_slices(painter, center, geometry, selection_levels, opacity, scale, accent);

        painter.circle_stroke(
            center,
            OUTER_RADIUS_DIP * scale,
            Stroke::new(1.5 * scale, faded(accent_color(accent, 210), opacity)),
        );
        painter.circle_stroke(
            center,
            INNER_RADIUS_DIP * scale,
            Stroke::new(scale, faded(accent_color(accent, 125), opacity)),
        );

        draw_slice_boundaries(painter, center, geometry, opacity, scale, accent);
        draw_slice_labels(painter, center, geometry, menu.labels(), opacity, scale, theme);

        painter.circle_filled(center, 28.0 * scale, faded(theme.center, opacity));
        painter.text(
            center,
            Align2::CENTER_CENTER,
            frame.activation_button.short_label(),
            FontId::proportional(11.0 * scale),
            faded(theme.center_label, opacity),
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
    accent: AccentColor,
) {
    for (index, segments) in geometry.selected_arcs.iter().enumerate() {
        let intensity = levels[index];
        if intensity <= f32::EPSILON {
            continue;
        }
        let stroke = Stroke::new(
            (RING_WIDTH_DIP - 5.0) * scale,
            faded(selected_accent_color(accent, 242), opacity * intensity),
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
    accent: AccentColor,
) {
    let stroke = Stroke::new(scale, faded(accent_color(accent, 88), opacity));
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
    theme: ThemeColors,
) {
    let color = faded(theme.label, opacity);
    for (offset, label) in menu_label_layout(geometry, labels) {
        let label_center = center + offset * scale;
        if let Some(background) = theme.label_background {
            let font = FontId::monospace(12.0 * scale);
            let galley = painter.layout_no_wrap(label.to_owned(), font, color);
            let label_rect = egui::Rect::from_center_size(label_center, galley.size())
                .expand2(Vec2::new(5.0, 2.5) * scale);
            painter.rect_filled(
                label_rect,
                egui::CornerRadius::same(4),
                faded(background, opacity),
            );
            painter.galley(label_rect.center() - galley.size() / 2.0, galley, color);
            continue;
        }
        painter.text(
            label_center,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThemeColors {
    ring: Color32,
    center: Color32,
    label: Color32,
    center_label: Color32,
    label_background: Option<Color32>,
}

fn theme_colors(theme: LabelTheme) -> ThemeColors {
    match theme {
        LabelTheme::Light => ThemeColors {
            ring: Color32::from_rgba_unmultiplied(18, 23, 35, 224),
            center: Color32::from_rgba_unmultiplied(8, 12, 20, 235),
            label: Color32::from_rgba_unmultiplied(225, 232, 255, 230),
            center_label: Color32::from_rgba_unmultiplied(225, 232, 255, 245),
            label_background: None,
        },
        LabelTheme::Dark => ThemeColors {
            ring: Color32::from_rgba_unmultiplied(232, 236, 245, 232),
            center: Color32::from_rgba_unmultiplied(246, 248, 252, 242),
            label: Color32::from_rgba_unmultiplied(24, 31, 44, 238),
            center_label: Color32::from_rgba_unmultiplied(24, 31, 44, 248),
            label_background: Some(Color32::from_rgba_unmultiplied(250, 251, 253, 232)),
        },
    }
}

fn accent_color(accent: AccentColor, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(accent.red, accent.green, accent.blue, alpha)
}

fn selected_accent_color(accent: AccentColor, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(
        accent.red.saturating_sub(33),
        accent.green.saturating_sub(19),
        accent.blue,
        alpha,
    )
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

    use gradial_core::{AccentColor, Appearance, LabelTheme, ResolvedMenu, SizePreset, SliceCount};

    use super::{
        FoundationOverlayRenderer, SelectionTransition, half_slice_angle, menu_label_layout,
        selected_accent_color, theme_colors,
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
            vec![None; 4].into(),
        )
        .expect("complete custom menu");
        let labels: Vec<_> =
            menu_label_layout(renderer.geometry.for_slice_count(menu.slice_count()), menu.labels())
                .map(|(_, label)| label)
                .collect();

        assert_eq!(labels, ["Build", "Search", "Tests", "Terminal"]);
    }

    #[test]
    fn default_appearance_preserves_foundation_colors_and_scale() {
        let appearance = Appearance::default();
        let theme = theme_colors(appearance.label_theme);

        assert_eq!(appearance.size_preset, SizePreset::Standard);
        assert_close(appearance.size_preset.visual_scale(), 1.0);
        assert_eq!(theme.ring, eframe::egui::Color32::from_rgba_unmultiplied(18, 23, 35, 224));
        assert_eq!(theme.label, eframe::egui::Color32::from_rgba_unmultiplied(225, 232, 255, 230));
        assert_eq!(
            selected_accent_color(appearance.accent_color, 242),
            eframe::egui::Color32::from_rgba_unmultiplied(91, 135, 255, 242)
        );
    }

    #[test]
    fn dark_labels_select_the_contrasting_light_surface() {
        let light = theme_colors(LabelTheme::Light);
        let dark = theme_colors(LabelTheme::Dark);

        assert!(u16::from(dark.ring.r()) > u16::from(light.ring.r()) + 150);
        assert!(dark.label.r() < light.label.r());
        assert!(light.label_background.is_none());
        assert!(dark.label_background.is_some());
        assert_eq!(
            selected_accent_color(AccentColor { red: 20, green: 10, blue: 5 }, 242),
            eframe::egui::Color32::from_rgba_unmultiplied(0, 0, 5, 242)
        );
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
