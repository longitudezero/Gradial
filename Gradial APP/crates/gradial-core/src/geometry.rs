use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

/// A point or vector in device-independent pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// The menu sizes supported by the v1 interaction contract.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SliceCount {
    Four,
    #[default]
    Eight,
}

impl SliceCount {
    #[must_use]
    pub const fn get(self) -> usize {
        match self {
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Four => "4",
            Self::Eight => "8",
        }
    }

    /// Returns a slice's center angle in radians, with north at zero and
    /// increasing angles moving clockwise.
    ///
    /// Returns `None` when `index` is outside this menu.
    #[must_use]
    pub fn center_angle(self, index: usize) -> Option<f32> {
        if index >= self.get() {
            return None;
        }

        let index = u8::try_from(index).ok()?;
        Some(match self {
            Self::Four => f32::from(index) * FRAC_PI_2,
            Self::Eight => f32::from(index) * FRAC_PI_4,
        })
    }

    /// Returns the screen-space unit vector for a slice center.
    ///
    /// Screen-space y increases downwards, so north has a y value of `-1`.
    #[must_use]
    pub fn unit_vector(self, index: usize) -> Option<Point> {
        self.center_angle(index).map(|angle| Point::new(angle.sin(), -angle.cos()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Point, SliceCount};

    const EPSILON: f32 = 0.000_01;

    fn assert_point(actual: Point, expected: Point) {
        assert!((actual.x - expected.x).abs() < EPSILON, "x: {actual:?}");
        assert!((actual.y - expected.y).abs() < EPSILON, "y: {actual:?}");
    }

    #[test]
    fn four_slice_centers_start_at_north_and_move_clockwise() {
        let expected = [
            Point::new(0.0, -1.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.0),
            Point::new(-1.0, 0.0),
        ];

        for (index, expected_point) in expected.into_iter().enumerate() {
            assert_point(SliceCount::Four.unit_vector(index).expect("valid slice"), expected_point);
        }
    }

    #[test]
    fn eight_slice_centers_include_diagonals() {
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        let expected = [
            Point::new(0.0, -1.0),
            Point::new(diagonal, -diagonal),
            Point::new(1.0, 0.0),
            Point::new(diagonal, diagonal),
            Point::new(0.0, 1.0),
            Point::new(-diagonal, diagonal),
            Point::new(-1.0, 0.0),
            Point::new(-diagonal, -diagonal),
        ];

        for (index, expected_point) in expected.into_iter().enumerate() {
            assert_point(
                SliceCount::Eight.unit_vector(index).expect("valid slice"),
                expected_point,
            );
        }
    }

    #[test]
    fn out_of_range_slice_is_rejected() {
        assert_eq!(SliceCount::Four.center_angle(4), None);
        assert_eq!(SliceCount::Eight.unit_vector(8), None);
    }
}
