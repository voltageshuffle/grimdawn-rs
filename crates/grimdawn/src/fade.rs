//! Fading a model out by its materials' `fade.x` factor (gibs, breakable rubble): the parts whose shader reads
//! the factor get per-instance material copies (the originals are shared with other models of the same mesh).
use bevy::prelude::*;
use gd_render::render::material::GdMaterial;

/// Swaps per-instance copies in for the `parts`' materials with any of `flags` in `params.flags.y` and returns
/// them. `None` (nothing swapped) when a part's material is not there yet and `wait` is set: retry next frame.
pub fn begin_fade(
    parts: impl IntoIterator<Item = Entity>,
    flags: u32,
    wait: bool,
    mats: &mut Query<&mut MeshMaterial3d<GdMaterial>>,
    materials: &mut Assets<GdMaterial>,
) -> Option<Vec<Handle<GdMaterial>>> {
    let mut copies = Vec::new();
    for p in parts {
        let m = mats.get(p).ok().and_then(|m| materials.get(&m.0));
        match m {
            Some(m) if m.params.flags.y & flags != 0 => copies.push((p, m.clone())),
            Some(_) => {}
            None if wait => return None,
            None => {}
        }
    }
    Some(
        copies
            .into_iter()
            .map(|(p, m)| {
                let h = materials.add(m);
                if let Ok(mut mat) = mats.get_mut(p) {
                    mat.0 = h.clone();
                }
                h
            })
            .collect(),
    )
}

/// Sets the copies' fade factor where it changed.
pub fn set_fade(copies: &[Handle<GdMaterial>], factor: f32, materials: &mut Assets<GdMaterial>) {
    for h in copies {
        if materials.get(h).is_some_and(|m| m.params.fade.x != factor)
            && let Some(mut m) = materials.get_mut(h)
        {
            m.params.fade.x = factor;
        }
    }
}
