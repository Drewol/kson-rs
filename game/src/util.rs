use three_d::{context::*, *};

pub fn back_pixels(context: &three_d::Context, viewport: Viewport) -> Vec<[u8; 4]> {
    unsafe {
        context.read_buffer(BACK);
    }
    let data_size = 4;
    let mut bytes = vec![0u8; viewport.width as usize * viewport.height as usize * data_size];
    unsafe {
        context.read_pixels(
            viewport.x,
            viewport.y,
            viewport.width as i32,
            viewport.height as i32,
            context::RGBA,
            context::UNSIGNED_BYTE,
            context::PixelPackData::Slice(&mut bytes),
        );
    }
    unsafe { bytes.align_to::<[u8; 4]>() }.1.to_vec()
}
