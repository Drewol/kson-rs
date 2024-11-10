use std::sync::mpsc::{self, Receiver, Sender};

use mlua::Lua;
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

pub fn lua_address(lua: &Lua) -> usize {
    let ptr = &*lua as *const _;
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
    use mlua::Lua;

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
