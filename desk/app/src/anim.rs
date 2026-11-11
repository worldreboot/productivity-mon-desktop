//! Posing a baked model: clips sampled into the nodes' places, turns and sizes, and those made the matrices its
//! vertices are moved by. No three.js and no gait here: the walk was baked (desk/bake/bake.ts).

use glam::{Mat4, Quat, Vec3};

use crate::mon::{Clip, Mon};

#[derive(Clone, Copy)]
pub struct Local {
    pub t: Vec3,
    pub r: Quat,
    pub s: Vec3,
}

pub struct Pose {
    pub locals: Vec<Local>,
    world: Vec<Mat4>,
    /// One matrix a joint, column-major, as the shader reads them.
    pub palette: Vec<[f32; 16]>,
}

impl Pose {
    pub fn new(mon: &Mon) -> Pose {
        let mut pose = Pose { locals: Vec::new(), world: vec![Mat4::IDENTITY; mon.header.nodes.len()], palette: vec![Mat4::IDENTITY.to_cols_array(); mon.header.joints.len()] };
        pose.rest(mon);
        pose
    }

    /// Every node back where the model has it at rest.
    pub fn rest(&mut self, mon: &Mon) {
        self.locals.clear();
        self.locals.extend(mon.header.nodes.iter().map(|n| Local { t: Vec3::from(n.t), r: Quat::from_array(n.r), s: Vec3::from(n.s) }));
    }

    /// Moves the nodes a clip moves toward where it has them at `time`, by `weight` of the way.
    pub fn apply(&mut self, mon: &Mon, clip: &Clip, time: f32, weight: f32) {
        if weight <= 0.0 || clip.frames < 2 {
            return;
        }
        let last = (clip.frames - 1) as f32;
        let along = if clip.looped { time.rem_euclid(clip.duration) } else { time.clamp(0.0, clip.duration) } / clip.duration * last;
        let frame = (along.floor() as usize).min(clip.frames - 2);
        let share = along - frame as f32;
        for track in &clip.tracks {
            let local = &mut self.locals[track.node];
            if track.property == 1 {
                let v = mon.floats(track.at + frame * 16, 8);
                let a = Quat::from_slice(&v[..4]);
                let b = Quat::from_slice(&v[4..]);
                // Neighbouring samples are a thirtieth of a second apart: a straight blend is as good as an arc.
                let b = if a.dot(b) < 0.0 { -b } else { b };
                let turn = a.lerp(b, share);
                local.r = if weight >= 1.0 { turn } else { local.r.lerp(if local.r.dot(turn) < 0.0 { -turn } else { turn }, weight) };
            } else {
                let v = mon.floats(track.at + frame * 12, 6);
                let value = Vec3::from_slice(&v[..3]).lerp(Vec3::from_slice(&v[3..]), share);
                let slot = if track.property == 0 { &mut local.t } else { &mut local.s };
                *slot = if weight >= 1.0 { value } else { slot.lerp(value, weight) };
            }
        }
    }

    /// Works out where every joint is from the nodes as they now stand.
    pub fn finish(&mut self, mon: &Mon) {
        for (i, node) in mon.header.nodes.iter().enumerate() {
            let local = self.locals[i];
            let own = Mat4::from_scale_rotation_translation(local.s, local.r, local.t);
            self.world[i] = if node.parent < 0 { own } else { self.world[node.parent as usize] * own };
        }
        for (i, joint) in mon.header.joints.iter().enumerate() {
            let inverse = Mat4::from_cols_array(&joint.inverse);
            self.palette[i] = if joint.node < 0 { inverse } else { self.world[joint.node as usize] * inverse }.to_cols_array();
        }
    }
}

/// How high the top of a mon is as it is posed, in its own units: the highest corner of the parts that show.
/// (A model's bounds are of everything in it at rest, shown or not, which can be a good deal taller.)
pub fn height(mon: &Mon, pose: &Pose) -> f32 {
    let (vertices, indices) = (mon.vertex_bytes(), mon.index_bytes());
    let float = |at: usize| f32::from_le_bytes(vertices[at..at + 4].try_into().unwrap());
    let mut top = f32::MIN;
    for group in &mon.header.groups {
        let shown = if group.show_node >= 0 { pose.locals[group.show_node as usize].t.x > 0.0 } else { group.shown };
        if !shown {
            continue;
        }
        for index in group.first as usize..(group.first + group.count) as usize {
            let at = u32::from_le_bytes(indices[index * 4..index * 4 + 4].try_into().unwrap()) as usize * crate::mon::STRIDE;
            let place = Vec3::new(float(at), float(at + 4), float(at + 8));
            // Moved by its joints as the shader moves it.
            let mut y = 0.0;
            for k in 0..4 {
                let joint = u16::from_le_bytes(vertices[at + 32 + k * 2..at + 34 + k * 2].try_into().unwrap()) as usize;
                y += Mat4::from_cols_array(&pose.palette[joint]).transform_point3(place).y * float(at + 40 + k * 4);
            }
            top = top.max(y);
        }
    }
    if top == f32::MIN { mon.header.bounds.max[1] } else { top }
}
