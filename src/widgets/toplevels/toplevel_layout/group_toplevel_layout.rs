use cosmic::iced::Length;

use super::LayoutToplevel;
use super::axis_toplevel_layout::{AxisPoint, AxisRectangle, AxisSize, AxisToplevelLayout};
use cosmic::iced::advanced::layout::flex::Axis;

/// Lays out all toplevels as one uniform grid: every window shares a single
/// scale factor, so the whole group scales up/down together like an app grid.
///
/// Rows are filled in order; the last row is only partially filled if the
/// window count doesn't divide evenly.
pub(crate) struct GroupToplevelLayout {
    spacing: f32,
}

impl GroupToplevelLayout {
    pub fn new(spacing: u32) -> Self {
        Self {
            spacing: f32::from(spacing as u16),
        }
    }

    /// Single scale factor for the whole grid: fit rows in the main axis,
    /// rows in the cross axis, never upscale past preferred size.
    fn scale_factor(
        &self,
        cols: usize,
        rows: usize,
        max_limit: AxisSize,
        toplevels: &[LayoutToplevel<'_, AxisSize>],
    ) -> f32 {
        let row_totals: Vec<f32> = (0..rows)
            .map(|row| {
                toplevels[row * cols..(row * cols + cols).min(toplevels.len())]
                    .iter()
                    .map(|t| t.preferred_size.main)
                    .sum::<f32>()
            })
            .collect();
        let max_row_total = row_totals
            .iter()
            .fold(f32::NEG_INFINITY, |a, &b| a.max(b))
            .max(1.0);
        let max_cross = toplevels
            .iter()
            .map(|t| t.preferred_size.cross)
            .fold(1.0_f32, f32::max);

        let total_main_spacing = self.spacing * (cols - 1) as f32;
        let total_cross_spacing = self.spacing * (rows - 1) as f32;
        let scale_main = (max_limit.main - total_main_spacing) / max_row_total;
        // Cross must fit the tallest cell in every row
        let scale_cross = (max_limit.cross - total_cross_spacing) / (max_cross * rows as f32);
        scale_main.min(scale_cross).min(1.)
    }
}

impl AxisToplevelLayout for GroupToplevelLayout {
    fn axis(&self) -> &Axis {
        &Axis::Horizontal
    }

    fn size(&self) -> AxisSize<Length> {
        AxisSize {
            main: Length::Fill,
            cross: Length::Fill,
        }
    }

    fn layout(
        &self,
        max_limit: AxisSize,
        toplevels: &[LayoutToplevel<'_, AxisSize>],
    ) -> impl Iterator<Item = AxisRectangle> {
        if toplevels.is_empty() {
            return Vec::new().into_iter();
        }

        // Choose the column count that maximizes the common scale factor.
        let mut cols = 1usize;
        let mut best_scale = f32::NEG_INFINITY;
        for candidate_cols in 1..=toplevels.len() {
            let candidate_rows = toplevels.len().div_ceil(candidate_cols);
            let candidate_scale =
                self.scale_factor(candidate_cols, candidate_rows, max_limit, toplevels);
            if candidate_scale.total_cmp(&best_scale) == std::cmp::Ordering::Greater {
                best_scale = candidate_scale;
                cols = candidate_cols;
            }
        }
        let rows = toplevels.len().div_ceil(cols);
        let scale_factor = self.scale_factor(cols, rows, max_limit, toplevels);

        // Scaled windows share one scale factor; each cell is as wide as its
        // window content, and rows are identical in cross size.
        let scaled_mains: Vec<f32> = toplevels
            .iter()
            .map(|t| t.preferred_size.main * scale_factor)
            .collect();
        let scaled_crosses: Vec<f32> = toplevels
            .iter()
            .map(|t| t.preferred_size.cross * scale_factor)
            .collect();
        let cell_cross = scaled_crosses.iter().fold(0_f32, |a, &b| a.max(b));
        // Total main-axis content of each row, including spacing
        let row_mains: Vec<f32> = (0..rows)
            .map(|row| {
                let start = row * cols;
                let count = cols.min(toplevels.len() - start);
                scaled_mains[start..start + count].iter().sum::<f32>()
                    + self.spacing * (count - 1) as f32
            })
            .collect();
        let grid_main = row_mains.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        let grid_cross = cell_cross * rows as f32 + self.spacing * (rows - 1) as f32;
        // Center the (possibly smaller) last row and the whole grid
        let padding_main = ((max_limit.main - grid_main) / 2.).max(0.);
        let padding_cross = ((max_limit.cross - grid_cross) / 2.).max(0.);

        let children: Vec<AxisRectangle> = toplevels
            .iter()
            .enumerate()
            .map(move |(i, t)| {
                let row = i / cols;
                let col = i % cols;
                let start = row * cols;
                // Center rows that have fewer windows than the grid width
                let row_content_main = row_mains[row];
                let row_padding_main = ((grid_main - row_content_main) / 2.).max(0.);
                let main_offset: f32 =
                    scaled_mains[start..i].iter().sum::<f32>() + col as f32 * self.spacing;

                AxisRectangle::new(
                    AxisPoint {
                        main: padding_main + row_padding_main + main_offset,
                        cross: padding_cross + row as f32 * (cell_cross + self.spacing),
                    },
                    AxisSize {
                        main: scaled_mains[i],
                        cross: t.preferred_size.cross * scale_factor,
                    },
                )
            })
            .collect();
        children.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::super::ToplevelLayout;
    use super::*;
    use cosmic::iced::{Rectangle, Size};
    use std::marker::PhantomData;

    fn toplevel(w: f32, h: f32) -> LayoutToplevel<'static> {
        LayoutToplevel {
            preferred_size: Size::new(w, h),
            _phantom_data: PhantomData,
        }
    }

    #[test]
    fn single_window_is_centered() {
        let layout = GroupToplevelLayout::new(16);
        let max = Size::new(1000., 800.);
        let rects: Vec<_> = ToplevelLayout::layout(&layout, max, &[toplevel(200., 300.)]).collect();
        assert_eq!(rects.len(), 1);
        let r = rects[0];
        assert_eq!(r.size(), Size::new(200., 300.));
        assert_eq!(r.x, (1000. - 200.) / 2.);
        assert_eq!(r.y, (800. - 300.) / 2.);
    }

    #[test]
    fn fits_within_limits() {
        let layout = GroupToplevelLayout::new(16);
        let max = Size::new(1000., 800.);
        let toplevels: Vec<_> = (0..7).map(|_| toplevel(400., 300.)).collect();
        for r in ToplevelLayout::layout(&layout, max, &toplevels) {
            assert!(r.x >= 0. && r.y >= 0.);
            assert!(r.x + r.width <= max.width + 0.001);
            assert!(r.y + r.height <= max.height + 0.001);
        }
    }

    #[test]
    fn uniform_scale() {
        let layout = GroupToplevelLayout::new(16);
        let max = Size::new(1000., 800.);
        let toplevels = vec![toplevel(600., 300.), toplevel(300., 200.)];
        let rects: Vec<_> = ToplevelLayout::layout(&layout, max, &toplevels).collect();
        let s0 = rects[0].size();
        let s1 = rects[1].size();
        assert!((s0.width / 600. - s1.width / 300.).abs() < 1e-3);
        assert!((s0.height / 300. - s1.height / 200.).abs() < 1e-3);
    }
}
