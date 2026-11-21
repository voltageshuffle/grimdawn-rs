use super::*;

/// The list's row order (sub_1400B0380).
pub(super) const ROW_ORDER: [u32; 67] = [
    63, 64, 65, 66, 1, 2, 3, 4, 56, 5, 6, 7, 8, 52, 53, 61, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
    19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 54, 55, 59, 35, 36, 37, 57, 38,
    39, 40, 41, 42, 43, 62, 44, 45, 46, 47, 48, 49, 50, 51, 58, 60, 67,
];

/// The list's rows: (action, function, primary, secondary).
pub(super) fn binding_rows(
    fonts: &FontCache,
    o: &GameOptions,
    b: &KeyBindings,
) -> Vec<(u32, String, String, String)> {
    let wasd = o.int("movementType") > 0;
    ROW_ORDER
        .iter()
        .filter(|a| wasd || !(63..=66).contains(*a))
        // ForceMove and Move are hidden in the keyboard modes (`sub_1401F73F0`).
        .filter(|a| !wasd || !(50..=51).contains(*a))
        .map(|&a| {
            let [p, s] = b.get(a);
            let tag = action_tag(a);
            (
                a,
                if tag.is_empty() {
                    String::new()
                } else {
                    fonts.tag(tag).trim().to_string()
                },
                key_name(p),
                key_name(s),
            )
        })
        .collect()
}
