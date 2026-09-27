use cosmic::iced::Length;

use super::LayoutToplevel;
use super::axis_toplevel_layout::{AxisPoint, AxisRectangle, AxisSize, AxisToplevelLayout};
use cosmic::iced::advanced::layout::flex::Axis;

/// Lays out all toplevels as one group that resembles the actual workspace
/// layout, like the GNOME overview:
///
/// - every window shares a single scale factor, so the whole group scales
///   up/down together and windows keep their relative sizes
/// - windows are packed edge-to-edge in rows, filling the whole available
///   area, instead of being arranged in a uniform grid
///
/// The scale factor is found via bisection so the packed group fits both
/// axes and uses as much of the area as possible. Windows are never scaled
/// up past their preferred size.
pub(crate) struct GroupToplevelLayout {
    spacing: f32,
}

#[derive(Default)]
struct PackedRow {
    start: usize,
    count: usize,
    extent_main: f32,
    extent_cross: f32,
}

impl GroupToplevelLayout {
    pub fn new(spacing: u32) -> Self {
        Self {
            spacing: f32::from(spacing as u16),
        }
    }

    /// Scale cap: never scale a window up past its preferred size, and keep
    /// each window at most as wide as the available area.
    fn scale_upper_bound(
        &self,
        max_limit: AxisSize,
        toplevels: &[LayoutToplevel<'_, AxisSize>],
    ) -> f32 {
        let max_main = toplevels
            .iter()
            .map(|t| t.preferred_size.main)
            .fold(1.0_f32, f32::max);
        (max_limit.main / max_main).min(1.).max(0.)
    }

    /// Pack windows at the given scale into rows that fit `max_limit.main`,
    /// returning the packed group's cross-axis size and the rows.
    fn packed_rows(
        &self,
        scale: f32,
        max_limit: AxisSize,
        toplevels: &[LayoutToplevel<'_, AxisSize>],
    ) -> (f32, Vec<PackedRow>) {
        let mut rows: Vec<PackedRow> = Vec::new();
        let mut cur = PackedRow::default();
        for (i, t) in toplevels.iter().enumerate() {
            let main = (t.preferred_size.main * scale).max(0.);
            let cross = (t.preferred_size.cross * scale).max(0.);
            let needs_new_row =
                cur.count > 0 && cur.extent_main + self.spacing + main > max_limit.main + 0.001;
            if needs_new_row {
                let full = std::mem::take(&mut cur);
                rows.push(full);
            }
            cur.extent_main = if cur.count > 0 {
                cur.extent_main + self.spacing + main
            } else {
                main
            };
            cur.extent_cross = cur.extent_cross.max(cross);
            if cur.count == 0 {
                cur.start = i;
                cur.count = 0;
            }
            cur.count += 1;
        }
        if cur.count > 0 {
            rows.push(cur);
        }

        let total_cross: f32 = rows.iter().map(|r| r.extent_cross).sum::<f32>()
            + self.spacing * (rows.len() as i32 - 1).max(0) as f32;
        (total_cross, rows)
    }

    /// Largest scale factor that still fits in the available area
    fn fitting_scale(
        &self,
        max_limit: AxisSize,
        toplevels: &[LayoutToplevel<'_, AxisSize>],
    ) -> f32 {
        let s_max = self.scale_upper_bound(max_limit, toplevels);
        if (self.packed_rows(s_max, max_limit, toplevels).0) <= max_limit.cross + 0.001 {
            return s_max;
        }
        // Bisection for the largest fitting scale
        let mut lo = 0.0f32;
        let mut hi = s_max;
        for _ in 0..32 {
            let mid = (lo + hi) / 2.;
            if self.packed_rows(mid, max_limit, toplevels).0 <= max_limit.cross + 0.001 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
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

        let scale_factor = self.fitting_scale(max_limit, toplevels);
        let (group_cross, rows) = self.packed_rows(scale_factor, max_limit, toplevels);

        // Center the group and center each row of differing width
        let padding_cross = ((max_limit.cross - group_cross) / 2.).max(0.);

        let mut cur_cross = padding_cross;
        let mut children = Vec::with_capacity(toplevels.len());
        for row in rows {
            let row_padding_main = ((max_limit.main - row.extent_main) / 2.).max(0.);
            let mut cur_main = row_padding_main;
            for t in &toplevels[row.start..row.start + row.count] {
                let main = t.preferred_size.main * scale_factor;
                let cross = t.preferred_size.cross * scale_factor;
                // Center windows vertically within the row
                let vertical_center = (row.extent_cross - cross).max(0.) / 2.;
                children.push(AxisRectangle::new(
                    AxisPoint {
                        main: cur_main,
                        cross: cur_cross + vertical_center,
                    },
                    AxisSize { main, cross },
                ));
                cur_main += main + self.spacing;
            }
            cur_cross += row.extent_cross + self.spacing;
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
