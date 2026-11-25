//! Shared fade machinery: a full-screen container whose children fade together.
use bevy::prelude::*;
use gd_ui_kit::Ui;

/// A full-screen container node whose children (image, solid and text nodes) fade together: the alpha applied to
/// them, on top of the alpha each was built with ([`BaseAlpha`]). Built with [`fade_group`], then
/// [`seal_fade_group`]; changed with [`Fader::set`].
#[derive(Component)]
pub struct FadeGroup(f32);

/// The alpha a [`FadeGroup`] child was built with.
#[derive(Component)]
struct BaseAlpha(f32);

/// A [`FadeGroup`] at `alpha`, a full-screen container under `parent`; `ui` builds into it from here on (build
/// the children with their own alpha, as if the group's were 1), then call [`seal_fade_group`].
pub fn fade_group(ui: &mut Ui, parent: Entity, alpha: f32) -> Entity {
    let e = ui
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            FadeGroup(alpha),
        ))
        .id();
    ui.commands.entity(parent).add_child(e);
    ui.root = e;
    e
}

/// After a [`fade_group`]'s children are spawned: remembers each child's own alpha and applies the group's, in
/// the same command flush as the spawns (the first frame already shows the faded colours).
pub fn seal_fade_group(commands: &mut Commands, group: Entity) {
    commands.queue(move |world: &mut World| {
        let (Some(alpha), Some(children)) = (
            world.get::<FadeGroup>(group).map(|g| g.0),
            world.get::<Children>(group).map(|c| c.to_vec()),
        ) else {
            return;
        };
        for c in children {
            let Ok(mut e) = world.get_entity_mut(c) else {
                continue;
            };
            // One colour per node, the same order as `Fader::set`: every `Node` carries a (default transparent)
            // `BackgroundColor`, so an image or text node's own colour comes first and its background is never faded in.
            let base = if let Some(mut n) = e.get_mut::<ImageNode>() {
                let base = n.color.alpha();
                n.color.set_alpha(base * alpha);
                base
            } else if let Some(mut n) = e.get_mut::<TextColor>() {
                let base = n.0.alpha();
                n.0.set_alpha(base * alpha);
                base
            } else if let Some(mut n) = e.get_mut::<BackgroundColor>() {
                let base = n.0.alpha();
                n.0.set_alpha(base * alpha);
                base
            } else {
                continue;
            };
            e.insert(BaseAlpha(base));
        }
    });
}

/// Changes a [`FadeGroup`]'s alpha in place.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Fader<'w, 's> {
    groups: Query<'w, 's, (&'static mut FadeGroup, &'static Children)>,
    #[allow(clippy::type_complexity)]
    colors: Query<
        'w,
        's,
        (
            &'static BaseAlpha,
            Option<&'static mut ImageNode>,
            Option<&'static mut BackgroundColor>,
            Option<&'static mut TextColor>,
        ),
    >,
}

impl Fader<'_, '_> {
    pub fn set(&mut self, group: Entity, alpha: f32) {
        let Ok((mut g, children)) = self.groups.get_mut(group) else {
            return;
        };
        if g.0 == alpha {
            return;
        }
        g.0 = alpha;
        for c in children.iter() {
            let Ok((base, image, solid, text)) = self.colors.get_mut(c) else {
                continue;
            };
            let a = base.0 * alpha;
            // The colour `seal_fade_group` based the alpha on, only.
            if let Some(mut n) = image {
                n.color.set_alpha(a);
            } else if let Some(mut n) = text {
                n.0.set_alpha(a);
            } else if let Some(mut n) = solid {
                n.0.set_alpha(a);
            }
        }
    }
}
