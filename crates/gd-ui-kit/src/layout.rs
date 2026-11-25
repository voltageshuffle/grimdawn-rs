//! Adaptive layout helpers: size columns, rows and tab segments to their content so localised
//! labels flow instead of overflowing. All widths and rects are physical pixels; the helpers only
//! measure and place, they spawn nothing.
//! PORT: the stock UI fixes column/row/tab sizes to their textures; here they follow the content.

use super::Ui;
use bevy::prelude::*;
use gd_formats::ui::Rect as URect;

/// One control row in a flow column: a left label of width `label_w` (0 when none) and a control
/// whose width shrinks from `ctrl_w` (natural) to `ctrl_min` (floor).
#[derive(Clone, Copy, Debug)]
pub struct FlowRow {
    pub label_w: f32,
    pub ctrl_w: f32,
    pub ctrl_min: f32,
}

impl FlowRow {
    pub fn fixed(label_w: f32, ctrl_w: f32) -> Self {
        Self {
            label_w,
            ctrl_w,
            ctrl_min: ctrl_w,
        }
    }

    pub fn shrink(label_w: f32, ctrl_w: f32, ctrl_min: f32) -> Self {
        Self {
            label_w,
            ctrl_w,
            ctrl_min,
        }
    }
}

/// The placed columns of [`flow_columns`]; `shrink` is the fit factor (1 = natural, 0 = floors).
#[derive(Clone, Debug)]
pub struct FlowLayout {
    pub col_x: Vec<f32>,
    pub col_w: Vec<f32>,
    pub label_w: Vec<f32>,
    pub label_gap: f32,
    pub shrink: f32,
}

impl FlowLayout {
    pub fn ctrl_x(&self, i: usize) -> f32 {
        self.col_x[i] + self.label_w[i] + self.label_gap
    }

    pub fn col_right(&self, i: usize) -> f32 {
        self.col_x[i] + self.col_w[i]
    }
}

/// A control's width at shrink factor `f`: linear from `min` (f=0) to `natural` (f=1).
pub fn shrink_w(natural: f32, min: f32, f: f32) -> f32 {
    min + (natural - min) * f
}

/// Lay out `cols` inside `content`, inset by `pad`: size each column to its widest row, find the
/// largest shrink factor whose columns fit, then spread them with a gap clamped to
/// `[col_gap_min, col_gap_max]`. Row y/height stay the caller's.
pub fn flow_columns(
    content: URect,
    pad: f32,
    cols: &[Vec<FlowRow>],
    label_gap: f32,
    col_gap_min: f32,
    col_gap_max: f32,
) -> FlowLayout {
    let n = cols.len();
    let usable = content.w - 2.0 * pad;
    // A control with no left label does not push the column.
    let label_w: Vec<f32> = cols
        .iter()
        .map(|col| col.iter().map(|r| r.label_w).fold(0.0, f32::max))
        .collect();
    // A left-label row aligns to the column's widest label; a control-only row does not.
    let row_w = |r: &FlowRow, lm: f32, f: f32| -> f32 {
        let left = if r.label_w > 0.0 { lm } else { 0.0 };
        left + label_gap + shrink_w(r.ctrl_w, r.ctrl_min, f)
    };
    let widths_at = |f: f32| -> Vec<f32> {
        (0..n)
            .map(|i| {
                cols[i]
                    .iter()
                    .map(|r| row_w(r, label_w[i], f))
                    .fold(0.0, f32::max)
            })
            .collect()
    };
    let fits = |w: &[f32]| -> bool {
        w.iter().sum::<f32>() + col_gap_min * n.saturating_sub(1) as f32 <= usable
    };
    // Binary search for the largest shrink factor that fits.
    let mut f = 1.0;
    if n > 1 && !fits(&widths_at(f)) {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..24 {
            let mid = (lo + hi) * 0.5;
            if fits(&widths_at(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        f = lo;
    }
    let col_w = widths_at(f);
    let total: f32 = col_w.iter().sum();
    let gap = if n > 1 {
        ((usable - total) / (n - 1) as f32).clamp(col_gap_min, col_gap_max)
    } else {
        0.0
    };
    let right = content.x + content.w - pad;
    let mut col_x = Vec::with_capacity(n);
    let mut x = content.x + pad;
    for w in &col_w {
        col_x.push(x);
        x += w + gap;
    }
    // A too-wide column is cut, not pushed out.
    let col_w: Vec<f32> = col_x
        .iter()
        .zip(col_w)
        .map(|(cx, w)| w.min(right - cx).max(0.0))
        .collect();
    FlowLayout {
        col_x,
        col_w,
        label_w,
        label_gap,
        shrink: f,
    }
}

/// The height of a row whose text wraps at `text_w`: the taller of `min_h` and the wrapped text.
pub fn row_height(ui: &mut Ui, style: &str, text: &str, text_w: f32, min_h: f32) -> f32 {
    let lines = ui.wrap_lines(style, text_w, text).len() as f32;
    min_h.max(lines * ui.line_height(style))
}

/// Size tab segments to their titles: each the wider of `min_w` and title + `2 * pad`, laid
/// contiguously from `origin`.
pub fn tab_strip(
    ui: &mut Ui,
    style: &str,
    titles: &[&str],
    origin: Vec2,
    h: f32,
    pad: f32,
    min_w: f32,
) -> Vec<URect> {
    let mut x = origin.x;
    titles
        .iter()
        .map(|t| {
            let w = min_w.max(ui.text_width(style, t) + 2.0 * pad);
            let r = URect::new(x, origin.y, w, h);
            x += w;
            r
        })
        .collect()
}
