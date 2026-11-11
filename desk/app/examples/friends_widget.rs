//! Offline captures of the shipping floating friends widget, including empty and overflow states.
#![allow(dead_code)]
#[path = "../src/variants.rs"] mod variants;
#[path = "../src/activity_icons.rs"] mod activity_icons;
#[path = "../src/app_icon.rs"] mod app_icon;
#[path = "../src/anim.rs"] mod anim;
#[path = "../src/mon.rs"] mod mon;
#[path = "../src/render.rs"] mod render;
#[path = "../src/look.rs"] mod look;
#[path = "../src/desk.rs"] mod desk;
#[path = "../src/panel.rs"] mod panel;
#[path = "../src/board.rs"] mod board;

use std::{fs, io::BufWriter, path::PathBuf};
use serde_json::json;

fn main() {
    let folder=PathBuf::from(std::env::args().nth(1).expect("output folder"));
    fs::create_dir_all(&folder).unwrap();
    let gpu=render::Gpu::headless();
    let format=wgpu::TextureFormat::Rgba8Unorm;
    for (case,count) in [("loading",None),("empty",Some(0)),("alone",Some(1)),("friends",Some(4)),("overflow",Some(30))] {
        let ctx=egui::Context::default();look::dress(&ctx);ctx.set_pixels_per_point(2.0);
        let names=["Maya","Dev trainer","A trainer with a very long name","Alex"];
        let social=count.map(|count|json!({"board":(0..count).map(|n|json!({"name":names[n%4],"you":n==1 || count==1,"xp":if n==0 {12450.0} else {1850.0/(n as f64)},"productive":3600,"mon":{"name":"Pikachu","level":12}})).collect::<Vec<_>>()})).unwrap_or(serde_json::Value::Null);
        let width=board::WIDE as u32;let mut height=500u32;
        let mut renderer=egui_wgpu::Renderer::new(&gpu.device,format,egui_wgpu::RendererOptions::default());
        for frame in 0..3 {
            let raw=egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(width as f32,height as f32))),..Default::default()};
            let (mut output,_,measured)=board::capture(&ctx,raw,&social);
            for (id,deltas) in &output.textures_delta.set {
                for delta in deltas { renderer.update_texture(&gpu.device,&gpu.queue,*id,delta); }
            }
            let size = [width*2,height*2];
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label:Some("website panel capture"),size:wgpu::Extent3d {width:size[0],height:size[1],depth_or_array_layers:1},
                mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,
                usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]
            });
            let target = texture.create_view(&Default::default());
            let jobs = ctx.tessellate(output.shapes,output.pixels_per_point);
            let screen = egui_wgpu::ScreenDescriptor {size_in_pixels:size,pixels_per_point:output.pixels_per_point};
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.update_buffers(&gpu.device,&gpu.queue,&mut encoder,&jobs,&screen);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {label:Some("native panel"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {view:&target,depth_slice:None,resolve_target:None,ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),store:wgpu::StoreOp::Store}})],..Default::default()}).forget_lifetime();
                renderer.render(&mut pass,&jobs,&screen);
            }
            let row = (size[0]*4).div_ceil(256)*256;
            let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {label:Some("panel readback"),size:(row*size[1]) as u64,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
            encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo {texture:&texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},wgpu::TexelCopyBufferInfo {buffer:&readback,layout:wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(row),rows_per_image:Some(size[1])}},wgpu::Extent3d {width:size[0],height:size[1],depth_or_array_layers:1});
            gpu.queue.submit([encoder.finish()]);
            if frame==2 {
                readback.slice(..).map_async(wgpu::MapMode::Read,|r|r.unwrap());
                gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let mapped = readback.slice(..).get_mapped_range().unwrap();
                let mut pixels:Vec<u8> = mapped.chunks_exact(row as usize).flat_map(|line|line[..size[0] as usize*4].to_vec()).collect();
                // The swapchain uses premultiplied alpha; PNG stores straight RGBA.
                for pixel in pixels.chunks_exact_mut(4) {
                    let alpha=pixel[3] as u32;
                    if alpha>0 && alpha<255 {for channel in &mut pixel[..3] {*channel=((*channel as u32*255+alpha/2)/alpha).min(255) as u8;}}
                }
                let mut png = png::Encoder::new(BufWriter::new(fs::File::create(folder.join(format!("{case}.png"))).unwrap()),size[0],size[1]);
                png.set_color(png::ColorType::Rgba);png.set_depth(png::BitDepth::Eight);
                png.set_compression(png::Compression::Best);
                png.write_header().unwrap().write_image_data(&pixels).unwrap();
            }
            for id in &output.textures_delta.free {renderer.free_texture(id);}
            output.textures_delta.clear();
            height=measured.ceil() as u32;
        }
        println!("Captured friends widget: {case} ({width}x{height})");
    }
}
