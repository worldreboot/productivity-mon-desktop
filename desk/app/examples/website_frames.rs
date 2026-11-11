//! Offline website animation exporter. Reuses the app's model, posing, camera,
//! lighting, and renderer; it never opens a window or starts activity tracking.
#![allow(dead_code)]
#[path = "../src/anim.rs"] mod anim;
#[path = "../src/mon.rs"] mod mon;
#[path = "../src/render.rs"] mod render;

use std::{fs, io::BufWriter, path::PathBuf, rc::Rc};
use glam::Mat4;
use serde_json::json;

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    fs::create_dir_all(&out).unwrap();
    let models = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
    let gpu = render::Gpu::headless();
    let mut metadata = Vec::new();
    const SIZE: usize = 256;
    const COLUMNS: usize = 8;
    for (name, dex) in [("eevee",133),("pikachu",25),("bulbasaur",1)] {
        let mon = Rc::new(mon::Mon::read(&models.join(format!("pm{dex:04}_00_00"))).unwrap());
        let mut model = gpu.model(mon.clone());
        let camera = model.camera();
        let walk = mon.clip("walk").unwrap();
        let pet = mon.clip("roar").or(mon.clip("attack")).unwrap();
        let walk_duration = mon.header.clips[walk].duration;
        let pet_duration = mon.header.clips[pet].duration;
        let mut clips = serde_json::Map::new();
        for (kind, frames, angles, duration) in [
            ("idle",72usize,vec![0.0],4.3),
            ("walk",16usize,vec![-1.2,-0.6,0.0,0.6,1.2],walk_duration),
            ("pet",48usize,vec![0.0],pet_duration),
        ] {
            let count = frames * angles.len();
            let rows = count.div_ceil(COLUMNS);
            let width = SIZE * COLUMNS;
            let mut atlas = vec![0u8; width * SIZE * rows * 4];
            for (direction, yaw) in angles.iter().enumerate() {
                for frame in 0..frames {
                    let time = frame as f32 / frames as f32 * duration;
                    let mut pose = anim::Pose::new(&mon);
                    if let Some(idle) = mon.clip("idle") { pose.apply(&mon,&mon.header.clips[idle],time,1.0); }
                    if let Some(eye) = mon.clip("eye") { pose.apply(&mon,&mon.header.clips[eye],time % 4.3,1.0); }
                    if kind == "walk" { pose.apply(&mon,&mon.header.clips[walk],time,1.0); }
                    if kind == "pet" {
                        let weight = (time / 0.15).min((duration-time)/0.15).clamp(0.0,1.0);
                        pose.apply(&mon,&mon.header.clips[pet],time,weight);
                    }
                    pose.finish(&mon);
                    let mut scene = [render::Item {model:&mut model,pose:&pose,place:Mat4::from_rotation_y(*yaw),flash:0.0,opacity:1.0}];
                    let mut pixels = gpu.snap((camera.0,camera.1,0.0,1.0),&mut scene,SIZE as u32,SIZE as u32);
                    // PNG/WebP use straight alpha; the native compositor uses premultiplied alpha.
                    for pixel in pixels.chunks_exact_mut(4) {
                        if pixel[3] > 0 && pixel[3] < 255 {
                            for channel in 0..3 { pixel[channel] = ((pixel[channel] as u32 * 255 + pixel[3] as u32/2) / pixel[3] as u32).min(255) as u8; }
                        }
                    }
                    let index = direction*frames+frame;
                    let (x,y) = ((index % COLUMNS)*SIZE,(index / COLUMNS)*SIZE);
                    for row in 0..SIZE {
                        let target = ((y+row)*width+x)*4;
                        atlas[target..target+SIZE*4].copy_from_slice(&pixels[row*SIZE*4..(row+1)*SIZE*4]);
                    }
                }
            }
            let path = out.join(format!("{name}-{kind}.png"));
            let mut encoder = png::Encoder::new(BufWriter::new(fs::File::create(path).unwrap()),width as u32,(rows*SIZE) as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&atlas).unwrap();
            clips.insert(kind.into(),json!({"src":format!("/assets/motion/{name}-{kind}.webp"),"frames":frames,"duration":duration,"angles":angles}));
            println!("{name} {kind}: {count} native frames");
        }
        metadata.push(json!({"name":name,"size":SIZE,"columns":COLUMNS,"feet":render::FEET,"pace":mon.header.clips[walk].speed*model.pixels_a_unit(SIZE as u32),"clips":clips}));
    }
    fs::write(out.join("manifest.json"),serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
}
