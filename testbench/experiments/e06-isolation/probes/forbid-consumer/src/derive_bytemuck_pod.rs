//! (d) `unsafe impl Pod`/`Zeroable` gerados por derive de uma crate real (`bytemuck_derive`).

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

pub fn pack(p: Pixel) -> u32 {
    bytemuck::cast(p)
}
