use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2, epaint::CircleShape};
use gradial_core::{
    AccentColor, ActivationBindingId, Appearance, ContinuousSelection, LabelTheme, ResolvedMenu,
    SliceCount,
};
use uuid::Uuid;

use crate::icons::{IconManager, IconVisual};

use super::animation::{AnimationMode, selection_progress};

const RING_RADIUS_DIP: f32 = 84.0;
const RING_WIDTH_DIP: f32 = 46.0;
const INNER_RADIUS_DIP: f32 = RING_RADIUS_DIP - RING_WIDTH_DIP / 2.0;
const OUTER_RADIUS_DIP: f32 = RING_RADIUS_DIP + RING_WIDTH_DIP / 2.0;
const LABEL_RADIUS_DIP: f32 = 84.0;
const ICON_RADIUS_DIP: f32 = 74.0;
const ICON_LABEL_RADIUS_DIP: f32 = 106.0;
const ICON_SIZE_DIP: f32 = 30.0;
const SELECTED_ARC_STEPS: u8 = 24;
const SELECTED_ARC_INSET_RADIANS: f32 = 0.035;
const CONTINUOUS_ARM_RADIUS_DIP: f32 = 48.0;
const CONTINUOUS_FULL_SCALE_RADIUS_DIP: f32 = 160.0;
const MAX_SLICE_COUNT: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct OverlayRenderFrame {
    pub(crate) invocation_id: Option<Uuid>,
    pub(crate) menu: Option<Arc<ResolvedMenu>>,
    pub(crate) activation_button: ActivationBindingId,
    pub(crate) appearance: Appearance,
    pub(crate) selected_slice: Option<usize>,
    pub(crate) continuous_selection: Option<ContinuousSelection>,
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
    icons: Arc<IconManager>,
    active_invocation: Option<Uuid>,
    selection: SelectionTransition,
}

impl Default for FoundationOverlayRenderer {
    fn default() -> Self {
        Self::new(Arc::new(IconManager::new(std::path::PathBuf::new())))
    }
}

impl FoundationOverlayRenderer {
    pub(crate) fn new(icons: Arc<IconManager>) -> Self {
        Self {
            geometry: RendererGeometryCaches::new(),
            icons,
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
        if let Some(selection) = frame.continuous_selection {
            draw_continuous_track(
                painter,
                center,
                menu.slice_count(),
                selection,
                opacity,
                scale,
                accent,
                theme,
            );
        }

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
        draw_slice_content(
            painter,
            center,
            geometry,
            menu,
            SliceContentStyle { opacity, scale, theme },
            &self.icons,
        );

        painter.circle_filled(center, 28.0 * scale, faded(theme.center, opacity));
        let center_label = frame.continuous_selection.map_or_else(
            || frame.activation_button.short_label().to_owned(),
            |selection| format!("{:.0}%", selection.value.clamp(0.0, 1.0) * 100.0),
        );
        painter.text(
            center,
            Align2::CENTER_CENTER,
            center_label,
            FontId::proportional(11.0 * scale),
            faded(theme.center_label, opacity),
        );

        selection_animating
    }
}

#[allow(clippy::cast_possible_truncation)]
fn continuous_value_f32(value: f64) -> f32 {
    value.clamp(0.0, 1.0) as f32
}

fn continuous_track_offsets(
    slice_count: SliceCount,
    selection: ContinuousSelection,
) -> Option<[Vec2; 3]> {
    let direction = slice_count.unit_vector(selection.slice_index)?;
    let direction = Vec2::new(direction.x, direction.y);
    let value = continuous_value_f32(selection.value);
    let current_radius = CONTINUOUS_ARM_RADIUS_DIP
        + (CONTINUOUS_FULL_SCALE_RADIUS_DIP - CONTINUOUS_ARM_RADIUS_DIP) * value;
    Some([
        direction * CONTINUOUS_ARM_RADIUS_DIP,
        direction * current_radius,
        direction * CONTINUOUS_FULL_SCALE_RADIUS_DIP,
    ])
}

#[allow(clippy::too_many_arguments)]
fn draw_continuous_track(
    painter: &egui::Painter,
    center: Pos2,
    slice_count: SliceCount,
    selection: ContinuousSelection,
    opacity: f32,
    scale: f32,
    accent: AccentColor,
    theme: ThemeColors,
) {
    let Some([start, current, end]) = continuous_track_offsets(slice_count, selection) else {
        return;
    };
    let start = center + start * scale;
    let current = center + current * scale;
    let end = center + end * scale;
    painter
        .line_segment([start, end], Stroke::new(6.0 * scale, faded(theme.center, opacity * 0.82)));
    painter.line_segment(
        [start, end],
        Stroke::new(2.0 * scale, faded(accent_color(accent, 105), opacity)),
    );
    painter.line_segment(
        [start, current],
        Stroke::new(4.0 * scale, faded(selected_accent_color(accent, 250), opacity)),
    );
    painter.circle_filled(current, 6.0 * scale, faded(selected_accent_color(accent, 255), opacity));
    painter.circle_stroke(
        current,
        7.5 * scale,
        Stroke::new(1.5 * scale, faded(theme.center_label, opacity)),
    );
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

fn draw_slice_content(
    painter: &egui::Painter,
    center: Pos2,
    geometry: &RendererGeometryCache,
    menu: &ResolvedMenu,
    style: SliceContentStyle,
    icons: &IconManager,
) {
    let SliceContentStyle { opacity, scale, theme } = style;
    for (index, (offset, label)) in menu_label_layout(geometry, menu.labels()).enumerate() {
        if let Some(reference) = menu.icon(index) {
            let (icon_offset, label_offset) = icon_and_label_offsets(offset);
            let icon_center = center + icon_offset * scale;
            let visual = icons.visual(reference);
            let icon_rect = fitted_icon_rect(icon_center, visual, ICON_SIZE_DIP * scale);
            match visual {
                IconVisual::Texture { texture_id, .. } => {
                    painter.image(
                        texture_id,
                        icon_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        faded(Color32::WHITE, opacity),
                    );
                }
                IconVisual::Placeholder => {
                    draw_icon_placeholder(painter, icon_rect, opacity, scale, theme);
                }
            }
            draw_label(
                painter,
                center + label_offset * scale,
                label,
                opacity,
                scale,
                theme,
                Some(theme.label_background.unwrap_or(theme.center)),
                10.0,
            );
            continue;
        }
        draw_label(
            painter,
            center + offset * scale,
            label,
            opacity,
            scale,
            theme,
            theme.label_background,
            12.0,
        );
    }
}

#[derive(Clone, Copy, Debug)]
struct SliceContentStyle {
    opacity: f32,
    scale: f32,
    theme: ThemeColors,
}

fn icon_and_label_offsets(label_offset: Vec2) -> (Vec2, Vec2) {
    let direction = label_offset / LABEL_RADIUS_DIP;
    (direction * ICON_RADIUS_DIP, direction * ICON_LABEL_RADIUS_DIP)
}

fn fitted_icon_rect(center: Pos2, visual: IconVisual, maximum: f32) -> Rect {
    let dimensions = match visual {
        IconVisual::Texture { dimensions, .. } => dimensions,
        IconVisual::Placeholder => [1, 1],
    };
    let width = f32::from(u16::try_from(dimensions[0]).unwrap_or(1));
    let height = f32::from(u16::try_from(dimensions[1]).unwrap_or(1));
    let scale = maximum / width.max(height).max(1.0);
    Rect::from_center_size(center, Vec2::new(width * scale, height * scale))
}

fn draw_icon_placeholder(
    painter: &egui::Painter,
    rect: Rect,
    opacity: f32,
    scale: f32,
    theme: ThemeColors,
) {
    painter.rect_filled(rect, egui::CornerRadius::same(5), faded(theme.center, opacity));
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(5),
        Stroke::new(1.5 * scale, faded(theme.label, opacity)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "?",
        FontId::proportional(16.0 * scale),
        faded(theme.label, opacity),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_label(
    painter: &egui::Painter,
    center: Pos2,
    label: &str,
    opacity: f32,
    scale: f32,
    theme: ThemeColors,
    background: Option<Color32>,
    font_size: f32,
) {
    let color = faded(theme.label, opacity);
    if let Some(background) = background {
        let font = FontId::monospace(font_size * scale);
        let galley = painter.layout_no_wrap(label.to_owned(), font, color);
        let label_rect =
            Rect::from_center_size(center, galley.size()).expand2(Vec2::new(5.0, 2.5) * scale);
        painter.rect_filled(label_rect, egui::CornerRadius::same(4), faded(background, opacity));
        painter.galley(label_rect.center() - galley.size() / 2.0, galley, color);
        return;
    }
    painter.text(center, Align2::CENTER_CENTER, label, FontId::monospace(font_size * scale), color);
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

    use gradial_core::{
        AccentColor, Appearance, ContinuousSelection, LabelTheme, ResolvedMenu, SizePreset,
        SliceCount,
    };

    use super::{
        FoundationOverlayRenderer, SelectionTransition, continuous_track_offsets, fitted_icon_rect,
        half_slice_angle, icon_and_label_offsets, menu_label_layout, selected_accent_color,
        theme_colors,
    };
    use crate::icons::IconVisual;
    use crate::overlay::AnimationMode;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0001);
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
        let renderer = FoundationOverlayRenderer::default();
        assert_eq!(renderer.cached_geometry_counts(SliceCount::Four), (4, 4, 4));
        assert_eq!(renderer.cached_geometry_counts(SliceCount::Eight), (8, 8, 8));
        assert!(renderer.geometry.four.selected_arcs.iter().all(|arc| arc.len() == 24));
        assert!(renderer.geometry.eight.selected_arcs.iter().all(|arc| arc.len() == 24));
    }

    #[test]
    fn renderer_layout_uses_the_resolved_profile_labels() {
        let renderer = FoundationOverlayRenderer::default();
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
    fn icon_layout_fits_aspect_ratio_and_moves_its_label_outward() {
        let (icon, label) = icon_and_label_offsets(eframe::egui::Vec2::new(0.0, -84.0));
        assert_close(icon.x, 0.0);
        assert_close(icon.y, -74.0);
        assert_close(label.y, -106.0);

        let rect = fitted_icon_rect(
            eframe::egui::Pos2::ZERO,
            IconVisual::Texture {
                texture_id: eframe::egui::TextureId::Managed(7),
                dimensions: [64, 32],
            },
            30.0,
        );
        assert_close(rect.width(), 30.0);
        assert_close(rect.height(), 15.0);
    }

    #[test]
    fn continuous_track_uses_arm_value_and_full_scale_radii() {
        let [start, current, end] = continuous_track_offsets(
            SliceCount::Eight,
            ContinuousSelection { slice_index: 2, value: 0.5 },
        )
        .expect("east continuous track");

        assert_close(start.x, 48.0);
        assert_close(start.y, 0.0);
        assert_close(current.x, 104.0);
        assert_close(current.y, 0.0);
        assert_close(end.x, 160.0);
        assert_close(end.y, 0.0);
        assert!(
            continuous_track_offsets(
                SliceCount::Four,
                ContinuousSelection { slice_index: 7, value: 0.5 }
            )
            .is_none()
        );
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
