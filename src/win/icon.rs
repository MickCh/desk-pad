//! The tray icon's pictures, drawn at run time: a coloured disc with a
//! white D-pad cross. No image files to ship.

use std::ptr::null_mut;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON,
};

/// An icon, destroyed when dropped.
pub struct Icon(pub HICON);

impl Drop for Icon {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: created by `CreateIconIndirect` and not shared.
            unsafe { DestroyIcon(self.0) };
        }
    }
}

/// Pixels (0xAARRGGBB, top row first) of a `size`×`size` disc in `rgb`
/// with a white cross. Straight (not premultiplied) alpha.
pub fn pixels(rgb: u32, size: u32) -> Vec<u32> {
    #[allow(clippy::cast_precision_loss)] // icon sizes are tiny
    let s = size as f32;
    let c = s / 2.0;
    let r = s * 0.47;
    let arm = r * 0.6;
    let half = (s / 10.0).max(1.0);
    let mut px = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            #[allow(clippy::cast_precision_loss)]
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let cover = (r - dx.hypot(dy) + 0.5).clamp(0.0, 1.0);
            let cross = (dx.abs() < half && dy.abs() < arm) || (dy.abs() < half && dx.abs() < arm);
            let color = if cross {
                0x00FF_FFFF
            } else {
                rgb & 0x00FF_FFFF
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let alpha = (cover * 255.0).round() as u32;
            px.push((alpha << 24) | color);
        }
    }
    px
}

/// The small-icon size of the current display settings.
pub fn small_size() -> u32 {
    // SAFETY: plain call without pointers.
    let n = unsafe { GetSystemMetrics(SM_CXSMICON) };
    u32::try_from(n).ok().filter(|&n| n >= 16).unwrap_or(16)
}

/// A disc icon in `rgb` (0xRRGGBB).
pub fn disc(rgb: u32, size: u32) -> Icon {
    let px = pixels(rgb, size);
    let side = i32::try_from(size).unwrap_or(16);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
            biWidth: side,
            biHeight: -side, // top row first
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    // Monochrome mask, all zero: the alpha channel decides. Rows are padded
    // to 16 bits.
    let mask_bits = vec![0u8; size.div_ceil(16) as usize * 2 * size as usize];
    // SAFETY: the DIB section has room for exactly `px.len()` pixels; both
    // bitmaps are deleted once the icon has copied them.
    unsafe {
        let mut bits = null_mut();
        let color = CreateDIBSection(
            null_mut(),
            &raw const info,
            DIB_RGB_COLORS,
            &raw mut bits,
            null_mut(),
            0,
        );
        if color.is_null() || bits.is_null() {
            return Icon(null_mut());
        }
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits.cast::<u32>(), px.len());
        let mask = CreateBitmap(side, side, 1, 1, mask_bits.as_ptr().cast());
        let info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&raw const info);
        DeleteObject(color);
        DeleteObject(mask);
        Icon(icon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_with_a_cross() {
        let px = pixels(0x2E9E4F, 32);
        assert_eq!(px.len(), 32 * 32);
        assert_eq!(px[0] >> 24, 0, "corner transparent");
        assert_eq!(px[16 * 32 + 16], 0xFFFF_FFFF, "centre: opaque white cross");
        assert_eq!(
            px[16 * 32 + 2],
            0xFF2E_9E4F,
            "inside the disc, off the cross"
        );
    }
}
