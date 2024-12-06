use std::sync::mpsc::{self, Receiver, Sender};

use femtovg::rgb::bytemuck;
use futures::executor::block_on;
use tealr::mlu::mlua::Lua;
use wgpu::{BufferUsages, ImageCopyBuffer, Origin3d, SurfaceTexture};

use crate::{help::RenderContext, Viewport};

pub fn back_pixels(
    context: &RenderContext,
    viewport: Viewport,
    surface: &SurfaceTexture,
) -> Vec<[u8; 4]> {
    let mut encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 0,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: true,
    });

    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTextureBase {
            texture: &surface.texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: None,
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: viewport.width,
            height: viewport.height,
            depth_or_array_layers: 0,
        },
    );
    let c = encoder.finish();
    let x = context.queue.submit([c]);
    let buffer_slice = buffer.slice(..);
    let (sender, reciever) = flume::bounded(1);
    buffer_slice.map_async(wgpu::MapMode::Read, move |f| sender.send(f).unwrap());

    context
        .device
        .poll(wgpu::MaintainBase::WaitForSubmissionIndex(x));

    block_on(reciever.recv_async())
        .ok()
        .map(|x| x.ok())
        .flatten()
        .expect("Could not read back buffer");

    let data = buffer_slice.get_mapped_range();
    let result = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    buffer.unmap();
    result
}

pub fn lua_address(lua: &Lua) -> usize {
    let ptr = &**lua as *const _;
    ptr as usize
}

#[allow(unused)]
pub struct Pipe<T, U> {
    rx: Receiver<T>,
    tx: Sender<U>,
}

#[allow(unused)]
impl<T, U> Pipe<T, U> {
    pub fn recv(&self) -> Result<T, std::sync::mpsc::RecvError> {
        self.rx.recv()
    }

    pub fn send(&self, message: U) -> Result<(), std::sync::mpsc::SendError<U>> {
        self.tx.send(message)
    }

    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Result<T, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    pub fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        self.rx.try_recv()
    }
}

#[allow(unused)]
pub fn pipe<T, U>() -> (Pipe<U, T>, Pipe<T, U>) {
    let (t_tx, t_rx) = mpsc::channel::<T>();
    let (u_tx, u_rx) = mpsc::channel::<U>();

    (Pipe { tx: t_tx, rx: u_rx }, Pipe { tx: u_tx, rx: t_rx })
}

#[cfg(test)]
mod tests {
    use tealr::mlu::mlua::Lua;

    use super::lua_address;

    #[test]
    fn lua_addresses() {
        let lua = &Lua::new();

        let a = lua_address(lua);
        let b = lua_address(lua);
        println!("{a}");
        assert!(a == b);
    }
}
