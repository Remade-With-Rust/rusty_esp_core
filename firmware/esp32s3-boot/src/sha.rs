//! SHA-256 on the chip's SHA unit, through the ROM, as `bootloader_sha256_*`
//! does it in the C loader.

use crate::rom::{self, ShaCtx};

pub struct Sha {
    ctx: ShaCtx,
}

impl Sha {
    pub const fn new() -> Self {
        Self { ctx: ShaCtx::zero() }
    }

    pub fn start(&mut self) {
        // SAFETY: ROM routines over this context.
        unsafe {
            rom::ets_sha_enable();
            rom::ets_sha_init(&mut self.ctx, rom::SHA2_256);
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        // SAFETY: the slice is valid for its length; `update_ctx = false`
        // is what the C loader passes.
        unsafe { rom::ets_sha_update(&mut self.ctx, data.as_ptr(), data.len() as u32, false) }
    }

    pub fn finish(&mut self) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: ROM routines over this context and a 32-byte output.
        unsafe {
            rom::ets_sha_finish(&mut self.ctx, out.as_mut_ptr());
            rom::ets_sha_disable();
        }
        out
    }
}
