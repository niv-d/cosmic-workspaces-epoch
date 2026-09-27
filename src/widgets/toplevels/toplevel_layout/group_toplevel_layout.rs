use cosmic::iced::Length;

use super::LayoutToplevel;
use super::axis_toplevel_layout::{AxisPoint, AxisRectangle, AxisSize, AxisToplevelLayout};
use cosmic::iced::advanced::layout::flex::Axis;

/// Lays out all toplevels as one group that resembles the actual workspace
/// layout, like the GNOME overview:
///
/// - every window shares a single scale factor, so the whole group scales
///   up/down together and windows keep their relative sizes
/// - windows are packed edge-to-edge, and rows hold at most ⌈√n⌉ windows,
///   filling the whole available area, instead of a uniform grid
///
/// The scale factor is the largest one fitting both axes with that
/// partitioning; windows are never scaled up past their preferred size.
pub(crate) struct GroupToplevelLayout {
    spacing: f32,
}

impl GroupToplevelLayout {
    pub fn new(spacing: u32) -> Self {
        Self {
            spacing: f32::from(spacing as u16),
        }
    }

    /// Largest scale factor that still fits in the available area with rows
    /// of up to ⌈√n⌉ windows.
    fn fitting_scale(
        &self,
        max_limit: AxisSize,
        toplevels: &[LayoutToplevel<'_, AxisSize>],
    ) -> f32 {
        let n = toplevels.len();
        let cols = (n as f32).sqrt().ceil() as usize;
        let rows = n.div_ceil(cols);

        let row_mains: Vec<f32> = (0..rows)
            .map(|row| {
                let start = row * cols;
                let count = cols.min(n - start);
                toplevels[start..start + count]
                    .iter()
                    .map(|t| t.preferred_size.main.max(0.))
                    .sum::<f32>()
            })
            .collect();
        let row_crosses: Vec<f32> = (0..rows)
            .map(|row| {
                let start = row * cols;
                let count = cols.min(n - start);
                toplevels[start..start + count]
                    .iter()
                    .map(|t| t.preferred_size.cross.max(0.))
                    .fold(0.0_f32, f32::max)
            })
            .collect();

        let max_row_main = row_mains
            .iter()
            .fold(f32::NEG_INFINITY, |a, &b| a.max(b))
            .max(1.0);
        let total_row_cross = row_crosses.iter().sum::<f32>().max(1.0);

        let scale_main = (max_limit.main - self.spacing * (cols - 1) as f32).max(0.) / max_row_main;
        let scale_cross =
            (max_limit.cross - self.spacing * (rows - 1) as f32).max(0.) / total_row_cross;
        scale_main.min(scale_cross).min(1.).max(0.)
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

        let n = toplevels.len();
        let cols = (n as f32).sqrt().ceil() as usize;
        let scale_factor = self.fitting_scale(max_limit, toplevels);

        // Extent of each row; the last row may hold fewer windows
        let row_extents: Vec<(f32, f32)> = (0..n.div_ceil(cols))
            .map(|row| {
                let start = row * cols;
                let count = cols.min(n - start);
                let slice = &toplevels[start..start + count];
                let main = slice
                    .iter()
                    .map(|t| t.preferred_size.main * scale_factor)
                    .sum::<f32>()
                    + self.spacing * (count - 1) as f32;
                let cross = slice
                    .iter()
                    .map(|t| t.preferred_size.cross * scale_factor)
                    .fold(0.0_f32, f32::max);
                (main, cross)
            })
            .collect();

        // Center the group and center smaller rows within the grid width
        let group_cross = row_extents.iter().map(|r| r.1).sum::<f32>()
            + self.spacing * (row_extents.len().saturating_sub(1)) as f32;
        let mut cur_cross = ((max_limit.cross - group_cross) / 2.).max(0.);

        let mut children = Vec::with_capacity(n);
        for (row, (row_extent_main, row_extent_cross)) in row_extents.into_iter().enumerate() {
            let row_top = cur_cross;
            let row_padding_main = ((max_limit.main - row_extent_main) / 2.).max(0.);
            let mut cur_main = row_padding_main;
            let start = row * cols;
            let count = cols.min(n - start);
            for t in &toplevels[start..start + count] {
                let main = t.preferred_size.main * scale_factor;
                let cross = t.preferred_size.cross * scale_factor;
                // Center windows vertically within the row
                let vertical_center = (row_extent_cross - cross).max(0.) / 2.;
                children.push(AxisRectangle::new(
                    AxisPoint {
                        main: cur_main,
                        cross: row_top + vertical_center,
                    },
                    AxisSize { main, cross },
                ));
                cur_main += main + self.spacing;
            }
            cur_cross = row_top + row_extent_cross + self.spacing;
        }
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
    fn three_windows_wrap_to_two_rows() {
        let layout = GroupToplevelLayout::new(16);
        let max = Size::new(1000., 800.);
        let toplevels: Vec<_> = (0..3).map(|_| toplevel(800., 400.)).collect();
        let rects: Vec<_> = ToplevelLayout::layout(&layout, max, &toplevels).collect();
        assert_eq!(rects.len(), 3);
        // First two share a row, the third wraps below and is centered
        assert_eq!(rects[0].y, rects[1].y);
        assert!(rects[2].y > rects[0].y);
        assert_eq!(rects[2].x, (1000. - rects[2].width) / 2.);
        assert!(rects[2].width > rects[0].width / 2.);
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

    #[test]
    fn fills_area() {
        let layout = GroupToplevelLayout::new(16);
        let max = Size::new(1000., 800.);
        let toplevels: Vec<_> = (0..4).map(|_| toplevel(2000., 1000.)).collect();
        let rects: Vec<_> = ToplevelLayout::layout(&layout, max, &toplevels).collect();
        // Optimal packing for four equal 2:1 windows is 2 per row at scale
        // 0.246: rows must span the full available width and use a good
        // fraction of the area
        let right = rects.iter().map(|r| r.x + r.width).fold(0., f32::max);
        assert!(right > max.width * 0.99);
        let used = rects.iter().map(|r| r.width * r.height).sum::<f32>();
        assert!(used / (max.width * max.height) > 0.5);
    }
}
