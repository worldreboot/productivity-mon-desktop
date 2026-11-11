//! A baked model, as desk/bake/bake.ts writes it: a JSON header and one blob of ready buffers.

// The header says more than the app reads yet: names are for people reading a model.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Deserialize)]
pub struct Node {
    pub name: String,
    /// Its parent among the nodes, always before it, or -1.
    pub parent: i32,
    pub t: [f32; 3],
    pub r: [f32; 4],
    pub s: [f32; 3],
}

#[derive(Deserialize)]
pub struct Joint {
    /// The node that moves it, or -1 for a part that is not skinned and stays where the model is.
    pub node: i32,
    pub inverse: [f32; 16],
}

#[derive(Deserialize)]
pub struct Span {
    pub at: usize,
    pub count: usize,
}

#[derive(Deserialize)]
pub struct Group {
    pub first: u32,
    pub count: u32,
    pub material: usize,
    pub shown: bool,
    /// The node that shows and hides it by where it is, or -1.
    #[serde(rename = "showNode")]
    pub show_node: i32,
}

#[derive(Deserialize)]
pub struct Material {
    pub name: String,
    pub texture: Option<String>,
    pub colour: [f32; 3],
    pub repeat: [f32; 2],
    pub offset: [f32; 2],
    /// 0 repeat, 1 clamp, 2 mirror, across and up.
    pub wrap: [u32; 2],
    pub blend: bool,
    #[serde(rename = "depthWrite")]
    pub depth_write: bool,
    #[serde(rename = "twoSided")]
    pub two_sided: bool,
    pub cut: f32,
    /// The node whose place moves the texture, or -1.
    #[serde(rename = "uvNode")]
    pub uv_node: i32,
    /// Shown as it is, with no light on it: a picture, not a body.
    #[serde(default)]
    pub unlit: bool,
}

#[derive(Deserialize)]
pub struct Track {
    pub node: usize,
    /// 0 place, 1 turn, 2 size.
    pub property: u8,
    pub at: usize,
}

#[derive(Deserialize)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    /// Samples in every track: the first at 0, the last at `duration`.
    pub frames: usize,
    #[serde(rename = "loop")]
    pub looped: bool,
    /// How fast it goes over the ground as it was made, in the model's units a second.
    pub speed: f32,
    pub tracks: Vec<Track>,
}

#[derive(Deserialize)]
pub struct Header {
    pub name: String,
    pub bounds: Bounds,
    pub nodes: Vec<Node>,
    pub joints: Vec<Joint>,
    pub vertices: Span,
    pub indices: Span,
    pub groups: Vec<Group>,
    pub materials: Vec<Material>,
    pub clips: Vec<Clip>,
}

/// Bytes in a vertex: place 3, normal 3, uv 2 (floats), joints 4 (u16), weights 4 (floats).
pub const STRIDE: usize = 56;

pub struct Mon {
    pub header: Header,
    /// The blob as 4-byte words, so that it can be read as floats where it lies.
    words: Vec<u32>,
    pub folder: PathBuf,
    /// A picture made in the app, not read from a file: what a material whose texture is "@pixels" shows. RGBA,
    /// top row first, with its width and height.
    pub pixels: Option<(Vec<u8>, u32, u32)>,
}

impl Mon {
    pub fn read(folder: &Path) -> Result<Mon, String> {
        let path = folder.join("model.mon");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if bytes.len() < 8 || &bytes[..4] != b"MON1" {
            return Err(format!("{} is not a baked model", path.display()));
        }
        let length = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let header: Header = serde_json::from_slice(&bytes[8..8 + length]).map_err(|e| format!("{}: {e}", path.display()))?;
        let words = bytes[8 + length..].chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        Ok(Mon { header, words, folder: folder.to_path_buf(), pixels: None })
    }

    /// A ball to throw, one unit in radius: red over a black band over white. Made here, not baked: it has no
    /// bones, and the one joint it has holds it where it is put.
    pub fn ball() -> Mon {
        const RINGS: usize = 16;
        const SLICES: usize = 24;
        // Where the band is, in rings from the top.
        const BAND: std::ops::Range<usize> = 7..9;
        let mut words: Vec<u32> = Vec::new();
        for ring in 0..=RINGS {
            let down = ring as f32 / RINGS as f32 * std::f32::consts::PI;
            for slice in 0..=SLICES {
                let round = slice as f32 / SLICES as f32 * std::f32::consts::TAU;
                let at = [down.sin() * round.cos(), down.cos(), down.sin() * round.sin()];
                // Place, normal (the same on a ball of one unit), uv, joints, weights.
                words.extend(at.iter().chain(&at).map(|v| v.to_bits()));
                words.extend([0, 0, 0, 0, 1f32.to_bits(), 0, 0, 0]);
            }
        }
        let count = (RINGS + 1) * (SLICES + 1);
        let mut groups = Vec::new();
        for (material, rings) in [(0, 0..BAND.start), (1, BAND), (2, BAND.end..RINGS)] {
            let first = (words.len() - count * STRIDE / 4) as u32;
            for ring in rings {
                for slice in 0..SLICES {
                    let (a, b) = ((ring * (SLICES + 1) + slice) as u32, ((ring + 1) * (SLICES + 1) + slice) as u32);
                    words.extend([a, a + 1, b, b, a + 1, b + 1]);
                }
            }
            groups.push(Group { first, count: (words.len() - count * STRIDE / 4) as u32 - first, material, shown: true, show_node: -1 });
        }
        let material = |name: &str, colour: [f32; 3]| Material {
            name: name.into(),
            texture: None,
            colour,
            repeat: [1.0, 1.0],
            offset: [0.0, 0.0],
            wrap: [0, 0],
            blend: false,
            depth_write: true,
            two_sided: false,
            cut: 0.0,
            uv_node: -1,
            unlit: false,
        };
        let header = Header {
            name: "ball".into(),
            bounds: Bounds { min: [-1.0; 3], max: [1.0; 3] },
            nodes: Vec::new(),
            joints: vec![Joint { node: -1, inverse: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] }],
            vertices: Span { at: 0, count },
            indices: Span { at: count * STRIDE, count: words.len() - count * STRIDE / 4 },
            groups,
            // Colours as light, not as the screen's numbers: the shader turns them.
            materials: vec![material("red", [0.78, 0.03, 0.03]), material("band", [0.02, 0.02, 0.02]), material("white", [0.88, 0.88, 0.88])],
            clips: Vec::new(),
        };
        Mon { header, words, folder: PathBuf::new(), pixels: None }
    }

    /// A flat picture to hold up: one unit wide and one tall, standing on y = 0 and facing the camera, showing
    /// `pixels` (RGBA, top row first) as they are, see-through where they are.
    pub fn plate(pixels: Vec<u8>, width: u32, height: u32) -> Mon {
        let mut words: Vec<u32> = Vec::new();
        // Place, normal, uv (bottom row is v = 0, as three.js has it), joints, weights.
        for (x, y) in [(-0.5f32, 0.0f32), (0.5, 0.0), (0.5, 1.0), (-0.5, 1.0)] {
            words.extend([x, y, 0.0, 0.0, 0.0, 1.0, x + 0.5, y].map(|v| v.to_bits()));
            words.extend([0, 0, 1f32.to_bits(), 0, 0, 0]);
        }
        words.extend([0, 1, 2, 0, 2, 3]);
        let material = Material {
            name: "plate".into(),
            texture: Some("@pixels".into()),
            colour: [1.0; 3],
            repeat: [1.0, 1.0],
            offset: [0.0, 0.0],
            wrap: [1, 1],
            blend: true,
            depth_write: false,
            two_sided: true,
            cut: 0.0,
            uv_node: -1,
            unlit: true,
        };
        let header = Header {
            name: "plate".into(),
            bounds: Bounds { min: [-0.5, 0.0, 0.0], max: [0.5, 1.0, 0.0] },
            nodes: Vec::new(),
            joints: vec![Joint { node: -1, inverse: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] }],
            vertices: Span { at: 0, count: 4 },
            indices: Span { at: 4 * STRIDE, count: 6 },
            groups: vec![Group { first: 0, count: 6, material: 0, shown: true, show_node: -1 }],
            materials: vec![material],
            clips: Vec::new(),
        };
        Mon { header, words, folder: PathBuf::new(), pixels: Some((pixels, width, height)) }
    }

    pub fn floats(&self, at: usize, count: usize) -> &[f32] {
        bytemuck::cast_slice(&self.words[at / 4..at / 4 + count])
    }

    pub fn vertex_bytes(&self) -> &[u8] {
        let span = &self.header.vertices;
        bytemuck::cast_slice(&self.words[span.at / 4..span.at / 4 + span.count * STRIDE / 4])
    }

    pub fn index_bytes(&self) -> &[u8] {
        let span = &self.header.indices;
        bytemuck::cast_slice(&self.words[span.at / 4..span.at / 4 + span.count])
    }

    pub fn clip(&self, name: &str) -> Option<usize> {
        self.header.clips.iter().position(|c| c.name == name)
    }
}
