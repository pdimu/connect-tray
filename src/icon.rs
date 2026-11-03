#![allow(unsafe_op_in_unsafe_fn, clippy::not_unsafe_ptr_arg_deref)]

use std::mem::size_of;
use std::ptr::null_mut;

use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HBITMAP};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO};

use crate::protocol::BatteryInfo;
use crate::state::BatteryTopology;

pub const ICON_RAW_SIZE: usize = 32 * 32 * 4;

static MASKS_BLOB: &[u8] = include_bytes!("../assets/masks.bin");

const DISCONNECTED_LIGHT_OFFSET: usize = 0;
const DISCONNECTED_DARK_OFFSET: usize = 4096;
const MASK_WH_OFFSET: usize = 8192;
const MASK_WF_L_OFFSET: usize = 9216;
const MASK_WF_R_OFFSET: usize = 10240;

pub fn is_system_light_theme() -> bool {
    let mut hkey: HKEY = null_mut();
    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let val_name: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();

    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return false;
        }

        let mut data = 0u32;
        let mut data_len = size_of::<u32>() as u32;
        let mut data_type = 0u32;
        let res = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            null_mut(),
            &mut data_type,
            &mut data as *mut u32 as *mut u8,
            &mut data_len,
        );
        RegCloseKey(hkey);
        res == 0 && data == 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorRgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl ColorRgb {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    pub fn lerp(self, other: Self, factor: f32) -> Self {
        Self {
            r: self.r + factor * (other.r - self.r),
            g: self.g + factor * (other.g - self.g),
            b: self.b + factor * (other.b - self.b),
        }
    }
}

pub fn compute_pixel_color(
    level: u8,
    is_light: bool,
    y: f32,
    y_top: f32,
    y_bottom: f32,
) -> ColorRgb {
    if level < 10 {
        return if is_light {
            ColorRgb::new(179.0, 38.0, 30.0) // #B3261E
        } else {
            ColorRgb::new(140.0, 29.0, 24.0) // #8C1D18
        };
    }

    let (top, bot) = if is_light {
        if level <= 30 {
            (
                ColorRgb::new(249.0, 222.0, 220.0), // #F9DEDC
                ColorRgb::new(179.0, 38.0, 30.0),   // #B3261E
            )
        } else if level <= 70 {
            (
                ColorRgb::new(242.0, 221.0, 184.0), // #F2DDB8
                ColorRgb::new(121.0, 90.0, 12.0),   // #795A0C
            )
        } else {
            (
                ColorRgb::new(209.0, 229.0, 206.0), // #D1E5CE
                ColorRgb::new(55.0, 106.0, 62.0),   // #376A3E
            )
        }
    } else {
        if level <= 30 {
            (
                ColorRgb::new(140.0, 29.0, 24.0),   // #8C1D18
                ColorRgb::new(242.0, 184.0, 181.0), // #F2B8B5
            )
        } else if level <= 70 {
            (
                ColorRgb::new(109.0, 80.0, 0.0),    // #6D5000
                ColorRgb::new(233.0, 193.0, 108.0), // #E9C16C
            )
        } else {
            (
                ColorRgb::new(29.0, 81.0, 40.0),    // #1D5128
                ColorRgb::new(156.0, 212.0, 159.0), // #9CD49F
            )
        }
    };

    let t = ((y - y_top) / (y_bottom - y_top)).clamp(0.0, 1.0);
    let t_cutoff = ((100 - level.min(100)) as f32 / 100.0).clamp(0.0, 0.9);

    if t <= t_cutoff {
        top
    } else if t >= 0.9 {
        bot
    } else {
        let span = 0.9 - t_cutoff;
        let factor = if span > 0.0001 {
            (t - t_cutoff) / span
        } else {
            1.0
        };
        top.lerp(bot, factor)
    }
}

pub fn apply_mask(
    bgra: &mut [u8; 32 * 32 * 4],
    mask: &[u8; 1024],
    level: u8,
    is_light: bool,
    y_top: f32,
    y_bottom: f32,
) {
    for y in 0..32 {
        let color = compute_pixel_color(level, is_light, y as f32, y_top, y_bottom);
        for x in 0..32 {
            let a = mask[y * 32 + x];
            if a > 0 {
                let a_norm = a as f32 / 255.0;
                let pr = (color.r * a_norm + 0.5) as u8;
                let pg = (color.g * a_norm + 0.5) as u8;
                let pb = (color.b * a_norm + 0.5) as u8;

                let idx = (y * 32 + x) * 4;
                bgra[idx + 0] = bgra[idx + 0].saturating_add(pb);
                bgra[idx + 1] = bgra[idx + 1].saturating_add(pg);
                bgra[idx + 2] = bgra[idx + 2].saturating_add(pr);
                bgra[idx + 3] = bgra[idx + 3].max(a);
            }
        }
    }
}

pub fn render_tray_icon(
    connected: bool,
    battery_topology: &Option<BatteryTopology>,
    single_battery: Option<BatteryInfo>,
    is_tws: bool,
) -> Option<HICON> {
    let light = is_system_light_theme();

    if !connected {
        let offset = if light {
            DISCONNECTED_LIGHT_OFFSET
        } else {
            DISCONNECTED_DARK_OFFSET
        };
        return create_hicon_from_bgra(&MASKS_BLOB[offset..offset + ICON_RAW_SIZE]);
    }

    let mut bgra = [0u8; ICON_RAW_SIZE];

    if is_tws {
        let (lvl_l, lvl_r) = match battery_topology {
            Some(BatteryTopology::Tws { left, right, .. }) => (left.level, right.level),
            _ => {
                let lvl = single_battery.map(|b| b.level).unwrap_or(100);
                (lvl, lvl)
            }
        };

        let mask_l: &[u8; 1024] = MASKS_BLOB[MASK_WF_L_OFFSET..MASK_WF_L_OFFSET + 1024]
            .try_into()
            .ok()?;
        let mask_r: &[u8; 1024] = MASKS_BLOB[MASK_WF_R_OFFSET..MASK_WF_R_OFFSET + 1024]
            .try_into()
            .ok()?;

        apply_mask(&mut bgra, mask_l, lvl_l, light, 7.2939, 24.7064);
        apply_mask(&mut bgra, mask_r, lvl_r, light, 7.2939, 24.7064);
    } else {
        let level = single_battery.map(|b| b.level).unwrap_or(100);
        let mask_wh: &[u8; 1024] = MASKS_BLOB[MASK_WH_OFFSET..MASK_WH_OFFSET + 1024]
            .try_into()
            .ok()?;
        apply_mask(&mut bgra, mask_wh, level, light, 4.04565, 27.9543);
    }

    create_hicon_from_bgra(&bgra)
}

pub fn create_hicon_from_bgra(pixels: &[u8]) -> Option<HICON> {
    if pixels.len() < ICON_RAW_SIZE {
        return None;
    }
    unsafe {
        let hbm_color: HBITMAP = CreateBitmap(
            32,
            32,
            1,
            32,
            pixels.as_ptr() as *const _,
        );
        if hbm_color.is_null() {
            return None;
        }

        let mask_bits = [0u8; 32 * 32 / 8];
        let hbm_mask: HBITMAP = CreateBitmap(
            32,
            32,
            1,
            1,
            mask_bits.as_ptr() as *const _,
        );
        if hbm_mask.is_null() {
            DeleteObject(hbm_color);
            return None;
        }

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };

        let h_icon = CreateIconIndirect(&icon_info);
        DeleteObject(hbm_color);
        DeleteObject(hbm_mask);

        if h_icon.is_null() {
            None
        } else {
            Some(h_icon)
        }
    }
}

pub fn destroy_icon_safe(icon: HICON) {
    if !icon.is_null() {
        unsafe {
            DestroyIcon(icon);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_colors() {
        // Full (green): 100% -> top color at y=4
        let c_100_top = compute_pixel_color(100, false, 4.04565, 4.04565, 27.9543);
        assert_eq!(c_100_top, ColorRgb::new(29.0, 81.0, 40.0));

        // Mid (amber): 50%
        let c_50_top = compute_pixel_color(50, false, 4.04565, 4.04565, 27.9543);
        assert_eq!(c_50_top, ColorRgb::new(109.0, 80.0, 0.0));

        // Low (pink): 20%
        let c_20_top = compute_pixel_color(20, false, 4.04565, 4.04565, 27.9543);
        assert_eq!(c_20_top, ColorRgb::new(140.0, 29.0, 24.0));

        // Crit (solid red): 5%
        let c_crit = compute_pixel_color(5, false, 15.0, 4.04565, 27.9543);
        assert_eq!(c_crit, ColorRgb::new(140.0, 29.0, 24.0));
    }

    #[test]
    fn test_blob_layout() {
        assert!(MASKS_BLOB.len() >= 11264);
    }
}
