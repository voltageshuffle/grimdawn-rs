//! Aiming ahead of a moving target (the monster AI's `LeadChance`).
//!
//! Game.dll `ControllerMonster::ComputeIntercept` (called by
//! `ControllerMonsterStateAttack::AttackEnemyOrReturn`): [`intercept`].

/// The point where a projectile launched from `from` at `speed` meets a
/// target at `target` moving with `velocity`, `delay` seconds of the
/// target's travel added (the launch delay). With d = target − from,
/// a = |v|² − s², b = 2 v·d, c = |d|²: |a| < 1e-4, a negative discriminant
/// or both roots negative: none; a zero discriminant: t = −b / 2a (no delay
/// added); else the smaller positive root (the positive one when only one
/// is) + `delay`. The point is target + v t. A still target is its own
/// point; a zero speed gives none.
pub fn intercept(
    target: [f32; 3],
    velocity: [f32; 3],
    from: [f32; 3],
    speed: f32,
    delay: f32,
) -> Option<[f32; 3]> {
    if speed == 0.0 {
        return None;
    }
    let v = velocity;
    if v == [0.0; 3] {
        return Some(target);
    }
    let d = [
        target[0] - from[0],
        target[1] - from[1],
        target[2] - from[2],
    ];
    let dot = |x: [f32; 3], y: [f32; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let a = dot(v, v) - speed * speed;
    if a.abs() < 1e-4 {
        return None;
    }
    let b = 2.0 * dot(v, d);
    let disc = b * b - 4.0 * dot(d, d) * a;
    if disc < 0.0 {
        return None;
    }
    let at = |t: f32| {
        Some([
            target[0] + v[0] * t,
            target[1] + v[1] * t,
            target[2] + v[2] * t,
        ])
    };
    if disc == 0.0 {
        return at(-0.5 / a * b);
    }
    let r = disc.sqrt();
    let t1 = (r - b) / (a + a);
    let t2 = (-b - r) / (a + a);
    if t1 < 0.0 && t2 < 0.0 {
        return None;
    }
    let t = if t1 > 0.0 {
        if t2 > 0.0 { t2.min(t1) } else { t1 }
    } else {
        t2
    };
    at(t + delay)
}
