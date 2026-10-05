//! What is turned: a tick, a sign that points the way a pattern goes, the axis of a mirror.
//!
//! Bevy's UI neither clips nor culls a node that is turned. It moves the corners of the node's
//! quad into the clip one by one, as if the node stood upright, and what comes of that for a
//! node outside the clip is a sliver drawn from where the node would be to the clip's edge:
//! over whatever lies above a list that was scrolled, say. So the kit sees to it that nothing
//! turned is drawn unless all of it lies within what clips it.

use bevy::{
    camera::visibility::VisibilitySystems,
    prelude::*,
    ui::{CalculatedClip, UiGlobalTransform, UiSystems},
};

/// The system that keeps what is turned from being drawn where it is not.
pub(super) fn plugin(app: &mut App) {
    // Once the frame's layout says where everything is, and after the visibilities that are
    // meant have been worked out, which this overrides.
    app.add_systems(PostUpdate, hide_turned.after(UiSystems::PostLayout).after(VisibilitySystems::VisibilityPropagate));
}

/// A node that is turned is drawn only while all of it is within what clips it: one that
/// crosses the edge is gone until it is back, rather than pulled out of shape. A node counts
/// as turned by what Bevy goes by, which clips and culls the others itself.
fn hide_turned(
    nodes: Query<(Entity, &UiGlobalTransform, &ComputedNode, Option<&CalculatedClip>, &Visibility, Option<&ChildOf>)>,
    mut drawn: Query<&mut InheritedVisibility>,
    mut turned: Local<Vec<(Entity, bool)>>,
) {
    turned.clear();
    for (entity, transform, node, clip, visibility, child_of) in &nodes {
        if transform.matrix2.x_axis.y == 0.0 {
            continue;
        }
        let half = 0.5 * node.size();
        let corners = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
        let within = clip.is_none_or(|clip| {
            corners.iter().all(|corner| clip.clip.contains(transform.transform_point2(*corner * half)))
        });
        // What its own visibility and its parent's come to, as Bevy works it out.
        let meant = match visibility {
            Visibility::Visible => true,
            Visibility::Hidden => false,
            Visibility::Inherited => {
                child_of.and_then(|child_of| drawn.get(child_of.parent()).ok()).is_none_or(|parent| parent.get())
            }
        };
        turned.push((entity, meant && within));
    }
    for &(entity, show) in turned.iter() {
        if let Ok(mut inherited) = drawn.get_mut(entity)
            && inherited.get() != show
        {
            *inherited = if show { InheritedVisibility::VISIBLE } else { InheritedVisibility::HIDDEN };
        }
    }
}
