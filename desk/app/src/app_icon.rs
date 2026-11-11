//! The same embedded mark for the app window, taskbar, and notification tray.
fn decode(bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info().expect("read the bundled app icon");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("decode the bundled app icon");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    (pixels, info.width, info.height)
}

pub fn window() -> winit::window::Icon {
    let (pixels, width, height) = decode(include_bytes!("../assets/productivitymon.png"));
    winit::window::Icon::from_rgba(pixels, width, height).expect("a valid app window icon")
}

pub fn tray_pixels() -> Vec<u8> {
    decode(include_bytes!("../assets/productivitymon-32.png")).0
}
