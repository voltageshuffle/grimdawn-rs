//! The CPU raster of the minimap / local map: draws the regions'
//! [`MinimapImage`]s into an RGBA8 buffer.
//!
//! `Grim Dawn.exe` `DetailMapWidget::DrawMapPass` (the detail-map widget's map pass):
//! every region's image is drawn as a ground quad over its bounding box
//! (`Region::GetBoundingBox`, region-local centre ± half extents in x / z)
//! with texture u 0 → 1 along +x and v 1 → 0 along +z (the image's top row is
//! the region's high-z edge), seen by an orthographic camera from above, with
//! `shaders/detailmapshader.ssh` (blend One / Zero, alpha 1: the image's
//! alpha, 255 on walkable ground and 0 elsewhere, is not used). Its second
//! texture is the fog mask: on the local map the region's session fog
//! (`Region::GetFogOfWar(region, true)`, `Region.sessionFog`: created fully fogged on
//! first use and revealed only by this session's `World::AddFOWVisibility`,
//! never saved; Engine.dll `Region::GetFogOfWar` / `World::AddFOWVisibility` / `FogOfWar::Reveal`,
//! which reveals it and the saved fog together), so ground explored in an
//! earlier session shows greyed until walked again. `FogOfWar::UpdateTexture`
//! (Engine.dll) puts the grid's byte in the texel's alpha
//! (255 = fogged) with the rows flipped (texture row 0 = the grid's last row =
//! low z); its coordinates are packed in the vertex colour (green u, blue v):
//! u from 1/w to 1 − 1/w along x, v from 1/h at low z to 1 − 1/h at high z
//! (w × h = the fog grid), i.e. grid row 0 (high z) at high z; on the HUD
//! minimap `system/textures/invisible.tex` (alpha 0). The DirectX11 pixel
//! shader greys the colour by the mask's alpha `a`:
//! `lerp(c, luma(c), saturate(2a − 1) × 2/3)`, luma = (0.2126, 0.7152,
//! 0.0722) · c. `DetailMapWidget::DrawFogPass` (the fog pass, after the map pass,
//! `DetailMapWidget.drawSavedFog`, set on both widgets): each region of the pass is drawn
//! again with its saved fog (`GetFogOfWar(region, false)`, `Region.savedFog`, which
//! always exists: `Region::LoadSavedFog` takes it from the `FOWManager` (`map.fow`)
//! or creates it fully fogged, so a region never visited is black) through
//! `shaders/fow.ssh` (black at alpha `a`, blend SrcAlpha / OneMinusSrcAlpha:
//! `c × (1 − a)`; its DirectX11 shader leaves the noise texture `fow.tex`
//! unread), on the minimap too, except regions of the running Shattered Realm
//! dungeon (`EndlessDungeon_Generator::IsRegionInDungeon(region, false)`):
//! there the local map only greys the ground unexplored this session and the
//! minimap shows it whole. [`RegionFog`] selects the passes.
//!
//! PORT: drawn on the CPU, bilinear (the image and the fog cells), rows spread across threads ([`draw_regions`]).
use gd_formats::map::minimap::MinimapImage;

/// A region's fog grid for [`draw_regions`]: `w × h` cells, 255 = fogged,
/// row 0 at the region's high-z edge.
pub struct FogMask<'a> {
    pub w: u32,
    pub h: u32,
    pub cells: &'a [u8],
}

impl FogMask<'_> {
    /// A grid with every cell fogged (a fog of war created for a region never visited).
    pub const FOGGED: FogMask<'static> = FogMask {
        w: 2,
        h: 2,
        cells: &[255; 4],
    };

    /// The fog texture's alpha 0..1 (1 = fogged) at region texture
    /// coordinates (u along x, z01 from low to high z), mapped as the
    /// engine's packed fog coordinates.
    fn alpha(&self, u: f32, z01: f32) -> f32 {
        if self.w < 2 || self.h < 2 || self.cells.len() < (self.w * self.h) as usize {
            return 1.0;
        }
        let (w, h) = (self.w as f32, self.h as f32);
        let fu = 1.0 / w + u * (1.0 - 2.0 / w);
        let fv = (1.0 - 1.0 / h) - z01 * (1.0 - 2.0 / h);
        let (fx, fy) = (fu * w - 0.5, fv * h - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let cell = |x: f32, y: f32| {
            let (x, y) = (
                (x as i32).clamp(0, self.w as i32 - 1) as u32,
                (y as i32).clamp(0, self.h as i32 - 1) as u32,
            );
            self.cells[(y * self.w + x) as usize] as f32
        };
        let f = (cell(x0, y0) * (1.0 - tx) + cell(x0 + 1.0, y0) * tx) * (1.0 - ty)
            + (cell(x0, y0 + 1.0) * (1.0 - tx) + cell(x0 + 1.0, y0 + 1.0) * tx) * ty;
        f / 255.0
    }
}

/// How a region's fog of war enters [`draw_region`].
pub struct RegionFog<'a> {
    /// The map pass's mask (local map: the session fog; the minimap's is
    /// `invisible.tex`, `None`): ground unexplored this session greyed.
    pub grey: Option<FogMask<'a>>,
    /// The `fow.ssh` pass's saved fog: unexplored ground darkened to black
    /// (`None` in the Shattered Realm dungeon).
    pub darken: Option<FogMask<'a>>,
}

impl RegionFog<'_> {
    /// The drawn colour of image colour `c` at region texture coordinates.
    fn apply(&self, c: [f32; 3], u: f32, z01: f32) -> [f32; 3] {
        let mut c = c;
        if let Some(m) = &self.grey {
            let g = (2.0 * m.alpha(u, z01) - 1.0).clamp(0.0, 1.0) * (2.0 / 3.0);
            let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            c = c.map(|v| v + g * (l - v));
        }
        if let Some(m) = &self.darken {
            let a = m.alpha(u, z01);
            c = c.map(|v| v * (1.0 - a));
        }
        c
    }
}

/// Screen → ground mapping of a top-down view: pixel (x, y)'s centre is at
/// world `origin + (x + 0.5) · dx + (y + 0.5) · dy` (x / z metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub origin: [f32; 2],
    pub dx: [f32; 2],
    pub dy: [f32; 2],
}

impl View {
    /// The world point (x, z) of screen point (`x`, `y`) px.
    pub fn world(&self, x: f32, y: f32) -> [f32; 2] {
        [
            self.origin[0] + x * self.dx[0] + y * self.dy[0],
            self.origin[1] + x * self.dx[1] + y * self.dy[1],
        ]
    }

    /// The screen point (px) of a world point.
    pub fn screen(&self, p: [f32; 2]) -> [f32; 2] {
        let det = self.dx[0] * self.dy[1] - self.dx[1] * self.dy[0];
        let (rx, rz) = (p[0] - self.origin[0], p[1] - self.origin[1]);
        [
            (rx * self.dy[1] - rz * self.dy[0]) / det,
            (self.dx[0] * rz - self.dx[1] * rx) / det,
        ]
    }
}

/// The image's bilinear RGB sample at texture coordinates (u, v).
fn sample(img: &MinimapImage, u: f32, v: f32) -> [f32; 3] {
    let (fx, fy) = (u * img.w as f32 - 0.5, v * img.h as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let cx = |x: f32| (x as i32).clamp(0, img.w as i32 - 1) as u32;
    let cy = |y: f32| (y as i32).clamp(0, img.h as i32 - 1) as u32;
    let px = |x: u32, y: u32| {
        let i = (y * img.w + x) as usize * 4;
        [
            img.rgba[i] as f32,
            img.rgba[i + 1] as f32,
            img.rgba[i + 2] as f32,
        ]
    };
    let (a, b, c, d) = (
        px(cx(x0), cy(y0)),
        px(cx(x0 + 1.0), cy(y0)),
        px(cx(x0), cy(y0 + 1.0)),
        px(cx(x0 + 1.0), cy(y0 + 1.0)),
    );
    std::array::from_fn(|k| {
        (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty
    })
}

/// One region for [`draw_regions`]: its world bounds (x, z min / max), image and fog.
pub struct RegionDraw<'a> {
    pub bounds: ([f32; 2], [f32; 2]),
    pub img: &'a MinimapImage,
    pub fog: Option<RegionFog<'a>>,
}

const ROWS_PER_THREAD: u32 = 48;

/// Fills `clip` (px: x0, y0, x1, y1, the ends exclusive) of `dst` (RGBA8, `dw × dh`) black and draws `regions` over
/// it in order, in row bands across threads.
pub fn draw_regions(
    dst: &mut [u8],
    (dw, dh): (u32, u32),
    clip: [u32; 4],
    view: &View,
    regions: &[RegionDraw],
) {
    let [x0, y0, x1, y1] = [clip[0], clip[1], clip[2].min(dw), clip[3].min(dh)];
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let stride = (dw * 4) as usize;
    let rows = &mut dst[y0 as usize * stride..y1 as usize * stride];
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get() as u32)
        .min((y1 - y0).div_ceil(ROWS_PER_THREAD))
        .max(1);
    let band = (y1 - y0).div_ceil(threads);
    let draw_band = |band_rows: &mut [u8], r0: u32| {
        let r1 = r0 + (band_rows.len() / stride) as u32;
        for y in r0..r1 {
            let s = (y - r0) as usize * stride;
            band_rows[s + x0 as usize * 4..s + x1 as usize * 4]
                .as_chunks_mut::<4>()
                .0
                .fill([0, 0, 0, 255]);
        }
        for r in regions {
            draw_region(band_rows, r0, dw, [x0, r0, x1, r1], view, r);
        }
    };
    if threads == 1 {
        draw_band(rows, y0);
        return;
    }
    std::thread::scope(|s| {
        for (k, chunk) in rows.chunks_mut(band as usize * stride).enumerate() {
            let r0 = y0 + k as u32 * band;
            s.spawn(move || draw_band(chunk, r0));
        }
    });
}

/// Draws one region's image into `dst` (RGBA8 rows from `row0` on, `dw` px wide, opaque) where it covers the view,
/// inside `clip` (px: x0, y0, x1, y1, the ends exclusive).
fn draw_region(dst: &mut [u8], row0: u32, dw: u32, clip: [u32; 4], view: &View, r: &RegionDraw) {
    let (min, max) = r.bounds;
    // The region's screen bounding box.
    let corners = [
        [min[0], min[1]],
        [max[0], min[1]],
        [min[0], max[1]],
        [max[0], max[1]],
    ]
    .map(|c| view.screen(c));
    let lo = |k: usize| corners.iter().map(|c| c[k]).fold(f32::INFINITY, f32::min);
    let hi = |k: usize| {
        corners
            .iter()
            .map(|c| c[k])
            .fold(f32::NEG_INFINITY, f32::max)
    };
    let (x0, x1) = (
        (lo(0).floor().max(clip[0] as f32)) as u32,
        (hi(0).ceil().min(clip[2] as f32)).max(0.0) as u32,
    );
    let (y0, y1) = (
        (lo(1).floor().max(clip[1] as f32)) as u32,
        (hi(1).ceil().min(clip[3] as f32)).max(0.0) as u32,
    );
    let (sx, sz) = (max[0] - min[0], max[1] - min[1]);
    let (du, dz) = (view.dx[0] / sx, view.dx[1] / sz);
    for y in y0..y1 {
        let p = view.world(x0 as f32 + 0.5, y as f32 + 0.5);
        let (mut u, mut z01) = ((p[0] - min[0]) / sx, (p[1] - min[1]) / sz);
        let row = ((y - row0) * dw) as usize * 4;
        for x in x0..x1 {
            if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&z01) {
                let mut c = sample(r.img, u, 1.0 - z01);
                if let Some(f) = &r.fog {
                    c = f.apply(c, u, z01);
                }
                let i = row + x as usize * 4;
                dst[i] = c[0] as u8;
                dst[i + 1] = c[1] as u8;
                dst[i + 2] = c[2] as u8;
                dst[i + 3] = 255;
            }
            u += du;
            z01 += dz;
        }
    }
}
