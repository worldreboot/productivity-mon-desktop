//! A small window that floats over everything and shows the friends' day: each friend and yourself, most
//! experience earned today first. It has no frame; it is moved by dragging it anywhere and shut by its cross (or
//! from the icon's menu). Like the panel it is egui on the app's own device, made when wanted and dropped when not.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

use crate::render::Gpu;

#[path = "board/avatars.rs"]
mod avatars;

/// How wide it is, and how tall its heading and each of its rows are, in logical pixels.
pub(crate) const WIDE: f32 = 280.0;
const ROW: f32 = 38.0;
const VISIBLE_ROWS: usize = 8;
/// How often the friends' day is asked for again while it is up.
pub const EVERY: Duration = Duration::from_secs(60);

/// What came of drawing it.
#[derive(Default)]
pub struct Said {
    /// Its cross was pressed.
    pub shut: bool,
    /// The friends' day is due to be asked for again.
    pub ask: bool,
    /// Open the Social page.
    pub social: bool,
    drag: bool,
}

/// Shared by the native window and offline layout checks.
fn contents(ui: &mut egui::Ui, social: &Value, avatars: &mut avatars::Avatars) -> (Said, f32) {
    use crate::look;
    let ink=egui::Color32::WHITE;
    let faint=egui::Color32::from_rgb(224,226,221);
    let good=egui::Color32::from_rgb(190,220,156);
    let mut said = Said::default();
    let board: Vec<&Value> = social["board"].as_array().into_iter().flatten().collect();
    let best = board.iter().filter_map(|row| row["xp"].as_f64()).fold(0.0, f64::max);
    let frame = egui::Frame::new().inner_margin(11).show(ui, |ui| {
        ui.set_width(WIDE-22.0);
        ui.spacing_mut().item_spacing = egui::vec2(6.0,6.0);
        let (head, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(),32.0),egui::Sense::hover());
        let close = egui::Rect::from_center_size(egui::pos2(head.right()-14.0,head.center().y),egui::vec2(28.0,28.0));
        let title = egui::Rect::from_min_max(head.min,egui::pos2(close.left()-6.0,head.bottom()));
        let title_response = ui.interact(title,ui.scope_id().with("social"),egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Open Social · drag to move");
        said.social = title_response.clicked();
        said.drag = title_response.drag_started();
        for x in [3.0,7.0] {for y in [-4.0,0.0,4.0] {ui.painter().circle_filled(egui::pos2(head.left()+x,head.center().y+y),0.9,faint);}}
        ui.painter().text(head.left_top()+egui::vec2(17.0,0.0),egui::Align2::LEFT_TOP,"Friends",egui::FontId::proportional(14.0),ink);
        ui.painter().text(head.left_bottom()+egui::vec2(17.0,0.0),egui::Align2::LEFT_BOTTOM,"Today's XP",egui::FontId::proportional(12.0),faint);
        let shut = ui.interact(close,ui.scope_id().with("close"),egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Hide · reopen from the tray menu");
        if shut.hovered() {ui.painter().rect_filled(close,6.0,egui::Color32::from_black_alpha(64));}
        let c=close.center();
        for direction in [-1.0,1.0] {ui.painter().line_segment([c+egui::vec2(-3.5,-3.5*direction),c+egui::vec2(3.5,3.5*direction)],egui::Stroke::new(1.3,faint));}
        said.shut = shut.clicked();
        // Preserve spacing without drawing a panel or a dividing line over the desktop.
        ui.add_space(7.0);
        if social.is_null() {
            ui.label(egui::RichText::new("Loading friends…").small().color(faint));
        } else if board.is_empty() {
            ui.label(egui::RichText::new("No activity yet today.").small().color(faint));
        } else {
            ui.style_mut().spacing.scroll=egui::style::ScrollStyle::solid();
            let visible=board.len().min(VISIBLE_ROWS);
            let list_height=ROW*visible as f32+4.0*visible.saturating_sub(1) as f32;
            egui::ScrollArea::vertical().id_salt("friends-rows").max_height(list_height).min_scrolled_height(list_height)
                .scroll_bar_visibility(if board.len()>VISIBLE_ROWS {egui::scroll_area::ScrollBarVisibility::AlwaysVisible} else {egui::scroll_area::ScrollBarVisibility::AlwaysHidden})
                .auto_shrink([false,true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y=4.0;
                for (rank,row) in board.iter().enumerate() {
                    let (line,hover)=ui.allocate_exact_size(egui::vec2(ui.available_width(),ROW),egui::Sense::hover());
                    let you=row["you"]==true;
                    let xp=row["xp"].as_f64().unwrap_or(0.0).max(0.0);
                    let center=line.center().y-1.0;
                    ui.painter().text(egui::pos2(line.left()+14.0,center),egui::Align2::CENTER_CENTER,format!("{}",rank+1),egui::FontId::proportional(12.0),faint);
                    let avatar=egui::Rect::from_center_size(egui::pos2(line.left()+42.0,center),egui::vec2(24.0,24.0));
                    avatars.show(ui,row["avatar"].as_str().unwrap_or_default(),avatar);
                    let name=row["name"].as_str().unwrap_or("Trainer");
                    let name=if you {format!("{name} · you")} else {name.to_string()};
                    let right=line.right()-8.0;
                    let xp_text=ui.painter().layout_no_wrap(xp_label(xp),egui::FontId::proportional(12.0),if you {good} else {faint});
                    let name_width=(right-xp_text.size().x-10.0-(line.left()+62.0)).max(0.0);
                    let mut job=egui::text::LayoutJob::simple(name,egui::FontId::proportional(14.0),ink,name_width);
                    job.wrap.max_rows=1;
                    job.wrap.break_anywhere=true;
                    job.wrap.overflow_character=Some('…');
                    let name_text=ui.painter().layout_job(job);
                    ui.painter().galley(egui::pos2(line.left()+62.0,center-name_text.size().y/2.0),name_text,ink);
                    ui.painter().galley(egui::pos2(right-xp_text.size().x,center-xp_text.size().y/2.0),xp_text,faint);
                    if best>0.0 && xp>0.0 {
                        let left=line.left()+62.0;
                        let bar=egui::Rect::from_min_size(egui::pos2(left,line.bottom()-4.0),egui::vec2((right-left)*(xp/best).clamp(0.0,1.0) as f32,2.0));
                        ui.painter().rect_filled(bar,1.0,if you {look::XP} else {egui::Color32::from_white_alpha(64)});
                    }
                    let minutes=(row["productive"].as_f64().unwrap_or(0.0)/60.0).floor() as i64;
                    let with=row["mon"]["nick"].as_str().or(row["mon"]["name"].as_str()).map(|name|format!("\nWith {name} · level {}",row["mon"]["level"])).unwrap_or_default();
                    hover.on_hover_text(format!("{}{}\n{xp:.0} XP · {minutes} productive min today{with}",row["name"].as_str().unwrap_or("Trainer"),if you {" (you)"} else {""}));
                }
            });
        }
        if !social.is_null() && board.len()<2 {
            said.social |= ui.add_sized([ui.available_width(),28.0],egui::Button::new(egui::RichText::new("Add friends").color(ink)).frame(false))
                .on_hover_text("Open Social to invite a friend").clicked();
        }
    });
    (said,frame.response.rect.bottom().max(frame.response.rect.height()))
}

fn xp_label(xp: f64) -> String {
    if xp>=1_000_000.0 {format!("{:.1}m XP",xp/1_000_000.0)}
    else if xp>=10_000.0 {format!("{:.1}k XP",xp/1_000.0)}
    else {format!("{xp:.0} XP")}
}

/// A small dark edge keeps the light overlay text legible on bright and dark windows alike.
fn text_shadows(output: &mut egui::FullOutput) {
    fn shade(shape: &mut egui::Shape) {
        match shape {
            egui::Shape::Vec(shapes) => { for shape in shapes { shade(shape); } }
            egui::Shape::Text(text) => {
                let color=text.galley.job.sections.first().map_or(text.fallback_color,|section|section.format.color);
                // Ordinary tooltips keep their dark type on the app's cream background.
                if color.r().max(color.g()).max(color.b()) < 180 {return;}
                let mut layers=Vec::with_capacity(5);
                for offset in [egui::vec2(-0.8,0.0),egui::vec2(0.8,0.0),egui::vec2(0.0,-0.8),egui::vec2(0.0,1.0)] {
                    let mut shadow=text.clone();
                    shadow.pos+=offset;
                    shadow.override_text_color=Some(egui::Color32::from_black_alpha(190));
                    layers.push(egui::Shape::Text(shadow));
                }
                layers.push(egui::Shape::Text(text.clone()));
                *shape=egui::Shape::Vec(layers);
            }
            _ => {},
        }
    }
    for shape in &mut output.shapes {shade(&mut shape.shape);}
}

#[cfg(any(test,feature = "website-capture"))]
pub(crate) fn capture(ctx: &egui::Context, raw: egui::RawInput, social: &Value) -> (egui::FullOutput,Said,f32) {
    let mut actions=Said::default();let mut height=0.0;
    let mut avatars=avatars::Avatars::offline();
    let mut output=ctx.run_ui(raw,|ui| {(actions,height)=contents(ui,social,&mut avatars);});
    text_shadows(&mut output);
    (output,actions,height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(ctx: &egui::Context, raw: egui::RawInput, social: &Value) -> (Said,f32) {
        let (mut output,said,height)=capture(ctx,raw,social);
        output.textures_delta.clear();
        (said,height)
    }

    #[test]
    fn content_fits_and_long_lists_scroll_at_a_bounded_height() {
        let ctx=egui::Context::default();crate::look::dress(&ctx);
        let raw=|| egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(WIDE,600.0))),..Default::default()};
        let social=|count| json!({"board":(0..count).map(|n|json!({"name":"A very long trainer name","xp":1234567,"you":n==0})).collect::<Vec<_>>()});
        let (_,alone)=frame(&ctx,raw(),&social(1));
        let (_,empty)=frame(&ctx,raw(),&social(0));
        let (_,many)=frame(&ctx,raw(),&social(40));
        assert!(alone>=120.0 && alone<165.0,"single-row content must include footer: {alone}");
        assert!(empty<alone);
        assert!(many<440.0,"large lists must stay on-screen: {many}");
        assert!(many>alone);
        // A previously short native window must grow when the social response arrives.
        let small=egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(WIDE,86.0))),..Default::default()};
        let (_,arrived)=frame(&ctx,small,&social(3));
        assert!(arrived>180.0,"new rows must not be constrained by the old height: {arrived}");
    }

    #[test]
    fn header_opens_social_and_close_only_hides_the_widget() {
        let ctx=egui::Context::default();crate::look::dress(&ctx);
        let social=serde_json::json!({"board":[]});
        let raw=|events|egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(WIDE,160.0))),events,..Default::default()};
        let _=frame(&ctx,raw(vec![]),&social);
        for (point,close) in [(egui::pos2(74.0,25.0),false),(egui::pos2(253.0,25.0),true)] {
            let event=|pressed|egui::Event::PointerButton {pos:point,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE};
            let _=frame(&ctx,raw(vec![egui::Event::PointerMoved(point),event(true)]),&social);
            let (said,_)=frame(&ctx,raw(vec![event(false)]),&social);
            assert_eq!(said.shut,close);
            assert_eq!(said.social,!close);
        }
    }
}

pub struct Board {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    ctx: egui::Context,
    input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    alpha_layer: Option<wgpu::TextureView>,
    /// When the friends' day was last asked for.
    asked: Option<Instant>,
    /// Measured content height; large friend lists scroll instead of extending beyond the desktop.
    height: u32,
    avatars: avatars::Avatars,
    /// When egui wants to be drawn again without anything having happened.
    pub again: Option<Instant>,
}

impl Board {
    /// Opens it where it was last left, or in the screen's top right corner.
    pub fn open(event_loop: &ActiveEventLoop, gpu: &Gpu, at: Option<(i32, i32)>, server: &str) -> Board {
        let attributes = Window::default_attributes()
            .with_title("Desk mons: friends")
            .with_window_icon(Some(crate::app_icon::window()))
            .with_inner_size(LogicalSize::new(WIDE, 140.0))
            .with_decorations(false)
            .with_transparent(true)
            
            .with_resizable(false)
            
            // Coming up, it does not take the keyboard from whatever is being worked in.
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnTop);
        let window = Arc::new(event_loop.create_window(crate::desk::overlay_attributes(attributes)).expect("a window"));
        #[cfg(target_os="windows")]
        let monitor = window.current_monitor().or(event_loop.primary_monitor());
        #[cfg(target_os="windows")]
        let place = match (at, &monitor) {
            // Where it was, if that is still on a screen.
            (Some((x, y)), _) if event_loop.available_monitors().any(|m| {
                let (from, size) = (m.position(), m.size());
                x >= from.x - 40 && y >= from.y && x < from.x + size.width as i32 - 40 && y < from.y + size.height as i32 - 40
            }) => Some((x, y)),
            (_, Some(monitor)) => {
                let gap = (24.0 * monitor.scale_factor()) as i32;
                Some((monitor.position().x + monitor.size().width as i32 - window.outer_size().width as i32 - gap, monitor.position().y + gap * 3))
            }
            _ => None,
        };
        #[cfg(target_os="macos")]
        let place={
            let screens=crate::desk::screens();
            at.filter(|(x,y)|screens.iter().any(|r|*x>=r.left&&*x<r.right&&*y>=r.top&&*y<r.bottom)).or_else(||{
                let screen=screens.first()?;let bounds=crate::desk::window_bounds(&window)?;
                let gap=(24.0*window.scale_factor()) as i32;
                Some((screen.right-(bounds.right-bounds.left)-gap,screen.top+gap))
            })
        };
        if let Some((x, y)) = place {
            crate::desk::position(&window,x,y);
        }
        keep_on_screen(&window);
        let (surface, config) = gpu.overlay(window.clone());
        let ctx = egui::Context::default();
        crate::look::dress(&ctx);
        let input = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, &*window, Some(window.scale_factor() as f32), None, None);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, config.format, egui_wgpu::RendererOptions::default());
        let alpha_layer=(config.alpha_mode==wgpu::CompositeAlphaMode::PostMultiplied).then(||gpu.overlay_layer(config.width,config.height,config.format));
        Board { window, surface, config, ctx, input, renderer, alpha_layer, asked: None, height: 140, avatars: avatars::Avatars::new(server), again: None }
    }

    /// Hears one of its window's events. Whether it should be drawn again for it.
    pub fn event(&mut self, gpu: &Gpu, event: &WindowEvent) -> bool {
        if let WindowEvent::Resized(size) = event {
            if size.width > 0 && size.height > 0 {
                (self.config.width, self.config.height) = (size.width, size.height);
                self.surface.configure(&gpu.device, &self.config);
                self.alpha_layer=(self.config.alpha_mode==wgpu::CompositeAlphaMode::PostMultiplied).then(||gpu.overlay_layer(size.width,size.height,self.config.format));
                keep_on_screen(&self.window);
            }
            return true;
        }
        self.input.on_window_event(&self.window, event).repaint
    }

    /// Draws it from what the server last said of the friends (null before it has).
    pub fn draw(&mut self, gpu: &Gpu, social: &Value) -> Said {
        let mut said = Said::default();
        if self.asked.is_none_or(|at| at.elapsed() >= EVERY) {
            self.asked = Some(Instant::now());
            said.ask = true;
        }
        self.avatars.update(&self.ctx,social);
        let raw = self.input.take_egui_input(&self.window);
        let mut output = self.ctx.run_ui(raw, |ui| {
            let (actions, height) = contents(ui, social, &mut self.avatars);
            said.shut = actions.shut;
            said.social = actions.social;
            said.drag = actions.drag;
            let height = height.ceil() as u32;
            if height != self.height {
                self.height = height;
                let _ = self.window.request_inner_size(LogicalSize::new(WIDE, height as f32));
            }
        });
        text_shadows(&mut output);
        if said.drag { let _ = self.window.drag_window(); }
        self.input.handle_platform_output(&self.window, output.platform_output);
        let delay = output.viewport_output.get(&egui::ViewportId::ROOT).map_or(Duration::MAX, |viewport| viewport.repaint_delay);
        self.again = Instant::now().checked_add(delay.min(EVERY));

        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        // As in the panel: the textures egui wants are made, then the list is emptied to say so.
        let mut textures = output.textures_delta;
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.config.width, self.config.height], pixels_per_point: output.pixels_per_point };
        if let wgpu::CurrentSurfaceTexture::Success(texture) | wgpu::CurrentSurfaceTexture::Suboptimal(texture) = self.surface.get_current_texture() {
            let target = texture.texture.create_view(&Default::default());
            let fill = wgpu::Color::TRANSPARENT;
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            self.renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
            {
                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("board"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: self.alpha_layer.as_ref().unwrap_or(&target),
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(fill), store: wgpu::StoreOp::Store },
                        })],
                        ..Default::default()
                    })
                    .forget_lifetime();
                self.renderer.render(&mut pass, &jobs, &screen);
            }
            if let Some(layer)=&self.alpha_layer {gpu.resolve_overlay(&mut encoder,layer,&target,self.config.format);}
            gpu.queue.submit([encoder.finish()]);
            self.window.pre_present_notify();
            gpu.queue.present(texture);
        }
        for id in &textures.free {
            self.renderer.free_texture(id);
        }
        textures.clear();
        said
    }
}

fn keep_on_screen(window: &Window) {
    let Some(bounds)=crate::desk::window_bounds(window) else {return;};
    let at=(bounds.left,bounds.top);
    let size=(bounds.right-bounds.left,bounds.bottom-bounds.top);
    let screens=crate::desk::screens();
    let Some(screen)=screens.iter().max_by_key(|r| {
        let width=(r.right.min(at.0+size.0)-r.left.max(at.0)).max(0) as i64;
        let height=(r.bottom.min(at.1+size.1)-r.top.max(at.1)).max(0) as i64;
        width*height
    }) else {return;};
    let x=at.0.clamp(screen.left+8,(screen.right-size.0-8).max(screen.left+8));
    let y=at.1.clamp(screen.top+8,(screen.bottom-size.1-8).max(screen.top+8));
    if (x,y)!=at {crate::desk::position(window,x,y);}
}
