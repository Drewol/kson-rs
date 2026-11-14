use di::RefMut;
use wgpu::CommandBuffer;

use super::Vgfx;

pub trait VgfxSyncExt {
    fn take_commands(&self) -> Vec<wgpu::CommandBuffer>;
    fn extend_commands(&self, commands: impl IntoIterator<Item = CommandBuffer>);
}

impl VgfxSyncExt for RefMut<Vgfx> {
    fn take_commands(&self) -> Vec<wgpu::CommandBuffer> {
        self.write().expect("Lock error").take_commands()
    }

    fn extend_commands(&self, commands: impl IntoIterator<Item = CommandBuffer>) {
        self.write().expect("Lock error").commands.extend(commands);
    }
}
