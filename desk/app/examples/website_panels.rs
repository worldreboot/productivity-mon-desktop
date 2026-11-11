//! Captures the actual desktop panel widgets into PNGs. Offline: no window, account, tracker, or HTTP.
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

use std::{collections::HashMap, fs, io::BufWriter, path::PathBuf};
use serde_json::json;

fn main() {
    let folder = PathBuf::from(std::env::args().nth(1).expect("output folder"));
    let icons_folder = PathBuf::from(std::env::args().nth(2).expect("installed icons folder"));
    let width: u32 = std::env::args().nth(3).map_or(400, |s| s.parse().expect("panel width"));
    let height: u32 = std::env::args().nth(4).map_or(400, |s| s.parse().expect("panel height"));
    let selected_page = std::env::args().nth(5);
    assert!(width >= 320 && height >= 320);
    fs::create_dir_all(&folder).unwrap();
    let gpu = render::Gpu::headless();
    let ctx = egui::Context::default();
    look::dress(&ctx);
    ctx.set_pixels_per_point(2.0);
    let state = json!({
        "account":{"id":"demo","name":"Trainer","anonymous":false},
        "points":960,"balls":8,"great":2,"ultra":0,"deskCap":3,"sorter":true,
        "prices":{"ball":50,"great":150,"ultra":400,"slot":500,"lure":200,"super-lure":350,"max-lure":450},"lure":null,
        "today":{"productive":5400.0,"goal":10800.0,"goalMet":false,"points":900,"xp":1800.0,"distracting":300.0},
        "streak":1500.0,"dayStreak":3,"session":null,"encounter":null,
        "season":{"name":"Bare Earth","type":"ground"},
        "mons":[{"id":"demo-mon","species":"bulbasaur","name":"Bulbasaur","nick":null,"dex":1,"level":12,"xp":90.0,"xpToNext":420.0,"types":["grass","poison"],"onDesk":true,"away":false,"genes":{"nature":19,"ivs":[27,8,27,11,0,8],"evs":[0,0,0,0,0,0]}}],
        "expeditions":[],"dex":{"seen":2,"caught":1,"total":1025,"entries":[{"species":"bulbasaur","name":"Bulbasaur","dex":1,"seen":true,"caught":true},{"species":"charmander","name":"Charmander","dex":4,"seen":true,"caught":false}]},
        "rules":{"code.exe":"productive","word.exe":"productive"}
    });
    let social = json!({"code":"DEMO1234","board":[],"friends":[],"incoming":[],"outgoing":[],"trades":[]});
    let ranks = HashMap::from([("/hiscores".to_string(),json!({"total":3,"rows":[
        {"rank":1,"owner":"Trainer A","level":42,"xp":18400.0,"mons":3,"you":false},
        {"rank":2,"owner":"Trainer B","level":28,"xp":11200.0,"mons":2,"you":false},
        {"rank":3,"owner":"Trainer","level":12,"xp":6300.0,"mons":1,"you":true}
    ],"yours":[]}))]);
    let empty = HashMap::new();
    let private = vec!["private-notes.exe".to_string()];
    let sites = Vec::new();
    let earning = serde_json::Value::Null;
    let status = json!({"online":true,"program":"word.exe","class":"productive","paused":false,"idle":false,"excluded":false,"syncAge":2,"nextReport":32});
    let mut icons = HashMap::new();
    let (pixels,w,h) = render::picture(&icons_folder.join("1.png")).expect("Bulbasaur icon");
    icons.insert(1,ctx.load_texture("icon-1",egui::ColorImage::from_rgba_unmultiplied([w as usize,h as usize],&pixels),egui::TextureOptions::LINEAR));
    for number in 2..=24 {
        if let Ok((pixels,w,h)) = render::picture(&icons_folder.join(format!("{number}.png"))) {
            icons.insert(number,ctx.load_texture(format!("icon-{number}"),egui::ColorImage::from_rgba_unmultiplied([w as usize,h as usize],&pixels),egui::TextureOptions::LINEAR));
        }
    }
    for number in 1..=24 {
        if let Ok((pixels,w,h)) = render::picture(&icons_folder.join("shiny").join(format!("{number}.png"))) {
            icons.insert(variants::key(number,true),ctx.load_texture(format!("shiny-icon-{number}"),egui::ColorImage::from_rgba_unmultiplied([w as usize,h as usize],&pixels),egui::TextureOptions::LINEAR));
        }
    }
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = egui_wgpu::Renderer::new(&gpu.device,format,egui_wgpu::RendererOptions::default());
    for page in ["welcome","focus","mons","mons-shiny","shop","shop-active","pokedex","pokedex-caught","pokedex-seen","pokedex-unseen","pokedex-search","pokedex-empty","social","privacy"] {
        if selected_page.as_deref().is_some_and(|selected| selected != page) { continue; }
        let mut state=state.clone();
        if page=="mons-shiny" {
            state["mons"][0]["shiny"]=json!(true);
            state["mons"][0]["species"]=json!("charizard");state["mons"][0]["name"]=json!("Charizard");state["mons"][0]["dex"]=json!(6);
            state["mons"][0]["types"]=json!(["fire","flying"]);state["mons"][0]["level"]=json!(36);
        }
        if page=="shop-active" { state["lure"]=json!({"kind":"max-lure","name":"Max Lure","remainingSeconds":2520.0,"totalSeconds":15000.0}); }
        let status = if page=="privacy" {json!({"online":true,"program":null,"class":null,"paused":true,"idle":false,"excluded":false,"syncAge":2,"nextReport":32})} else {status.clone()};
        let switches = [("track","Pause tracking",page=="privacy"),("hide","Hide mons",false)];
        let view = panel::View {state:&state,status:&status,account:true,note:"",sites:&sites,private:&private,social:&social,theirs:&empty,ranks:&ranks,earning:&earning,switches:&switches};
        // egui lays out some panels on the first frame; retain textures and render its settled frame.
        for frame in 0..3 {
            let mut output = panel::capture::frame(&ctx,page,&view,&icons,egui::vec2(width as f32,height as f32));
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
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {label:Some("native panel"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {view:&target,depth_slice:None,resolve_target:None,ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color::BLACK),store:wgpu::StoreOp::Store}})],..Default::default()}).forget_lifetime();
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
                let pixels:Vec<u8> = mapped.chunks_exact(row as usize).flat_map(|line|line[..size[0] as usize*4].to_vec()).collect();
                let mut png = png::Encoder::new(BufWriter::new(fs::File::create(folder.join(format!("{page}.png"))).unwrap()),size[0],size[1]);
                png.set_color(png::ColorType::Rgba);png.set_depth(png::BitDepth::Eight);
                png.set_compression(png::Compression::Best);
                png.write_header().unwrap().write_image_data(&pixels).unwrap();
            }
            for id in &output.textures_delta.free {renderer.free_texture(id);}
            output.textures_delta.clear();
        }
        println!("Captured actual app panel: {page}");
    }
}
