//! DXGI separate pointer composition, before the viewport scales/pans the frame.
use anyhow::{bail, ensure, Result};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Dxgi::DXGI_OUTDUPL_POINTER_SHAPE_INFO;

use crate::capture::CapturedFrame;

const MONOCHROME: u32 = 1;
const COLOR: u32 = 2;
const MASKED_COLOR: u32 = 4;

pub struct PointerShape {
    info: DXGI_OUTDUPL_POINTER_SHAPE_INFO,
    bytes: Vec<u8>,
    height: u32,
}

impl PointerShape {
    pub fn new(info: DXGI_OUTDUPL_POINTER_SHAPE_INFO, bytes: Vec<u8>) -> Result<Self> {
        let row_bytes = match info.Type {
            MONOCHROME => {
                ensure!(info.Height.is_multiple_of(2), "odd monochrome pointer mask height");
                u64::from(info.Width).div_ceil(8)
            }
            COLOR | MASKED_COLOR => u64::from(info.Width) * 4,
            other => bail!("unsupported DXGI pointer shape type: {other}"),
        };
        ensure!(info.Width > 0 && info.Height > 0, "empty pointer shape");
        ensure!(u64::from(info.Pitch) >= row_bytes, "invalid pointer pitch");
        ensure!(u64::from(info.Pitch) * u64::from(info.Height) <= bytes.len() as u64,
            "truncated pointer shape");
        let height = if info.Type == MONOCHROME { info.Height / 2 } else { info.Height };
        Ok(Self { info, bytes, height })
    }

    pub fn composite(&self, frame: &mut CapturedFrame, position: POINT) {
        // DXGI supplies the output-local top-left, not the hotspot. Negative
        // coordinates are valid when part of the cursor extends off the output.
        let left = i64::from(position.x);
        let top = i64::from(position.y);
        let x_start = left.max(0).min(i64::from(frame.width));
        let y_start = top.max(0).min(i64::from(frame.height));
        let x_end = (left + i64::from(self.info.Width)).clamp(0, i64::from(frame.width));
        let y_end = (top + i64::from(self.height)).clamp(0, i64::from(frame.height));
        let pitch = self.info.Pitch as usize;
        for y in y_start..y_end {
            let sy = (y - top) as usize;
            for x in x_start..x_end {
                let sx = (x - left) as usize;
                let offset = y as usize * frame.stride as usize + x as usize * 4;
                let dst = &mut frame.pixels[offset..offset + 4];
                if self.info.Type == MONOCHROME {
                    let bit = 0x80 >> (sx % 8);
                    let and_mask = if self.bytes[sy * pitch + sx / 8] & bit != 0 { 255 } else { 0 };
                    let xor_mask = if self.bytes[(sy + self.height as usize) * pitch + sx / 8] & bit != 0 { 255 } else { 0 };
                    for channel in &mut dst[..3] { *channel = (*channel & and_mask) ^ xor_mask; }
                } else {
                    let source = sy * pitch + sx * 4;
                    let src = &self.bytes[source..source + 4];
                    for channel in 0..3 {
                        dst[channel] = if self.info.Type == MASKED_COLOR {
                            if src[3] == 0 { src[channel] } else { dst[channel] ^ src[channel] }
                        } else {
                            let alpha = u32::from(src[3]);
                            ((u32::from(src[channel]) * alpha + u32::from(dst[channel]) * (255 - alpha) + 127) / 255) as u8
                        };
                    }
                }
                // A desktop pixel remains opaque, including XOR/masked cursors.
                dst[3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32) -> CapturedFrame {
        CapturedFrame { width, height, stride: width * 4,
            pixels: [10, 20, 30, 255].repeat((width * height) as usize), frame_id: 1 }
    }

    fn shape(kind: u32, width: u32, height: u32, pitch: u32, bytes: Vec<u8>) -> PointerShape {
        PointerShape::new(DXGI_OUTDUPL_POINTER_SHAPE_INFO {
            Type: kind, Width: width, Height: height, Pitch: pitch,
            HotSpot: POINT { x: 7, y: 9 }, // Must not affect placement.
        }, bytes).unwrap()
    }

    #[test]
    fn color_blends_bgra_and_honors_pitch_and_output_local_position() {
        let pointer = shape(COLOR, 1, 2, 8,
            vec![110, 120, 130, 128, 99, 99, 99, 99, 200, 210, 220, 255, 99, 99, 99, 99]);
        let mut output = frame(3, 2);
        pointer.composite(&mut output, POINT { x: 1, y: 0 });
        assert_eq!(&output.pixels[4..8], &[60, 70, 80, 255]);
        assert_eq!(&output.pixels[16..20], &[200, 210, 220, 255]);
        assert_eq!(&output.pixels[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn monochrome_supports_black_white_transparent_and_inverted_pixels() {
        // AND bits: 0 0 1 1; XOR bits: 0 1 0 1. Height includes both masks.
        let pointer = shape(MONOCHROME, 4, 2, 1, vec![0b00110000, 0b01010000]);
        let mut output = frame(4, 1);
        pointer.composite(&mut output, POINT::default());
        assert_eq!(output.pixels, vec![0, 0, 0, 255, 255, 255, 255, 255,
            10, 20, 30, 255, 245, 235, 225, 255]);
    }

    #[test]
    fn masked_color_replaces_or_xors_instead_of_alpha_blending() {
        let pointer = shape(MASKED_COLOR, 2, 1, 8, vec![1, 2, 3, 0, 255, 0, 255, 255]);
        let mut output = frame(2, 1);
        pointer.composite(&mut output, POINT::default());
        assert_eq!(output.pixels, vec![1, 2, 3, 255, 245, 20, 225, 255]);
    }

    #[test]
    fn clips_negative_coordinates_and_all_screen_edges() {
        let pointer = shape(COLOR, 2, 2, 8,
            vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 11, 12, 13, 255]);
        let mut output = frame(2, 2);
        pointer.composite(&mut output, POINT { x: -1, y: -1 });
        assert_eq!(&output.pixels[..4], &[11, 12, 13, 255]);
        assert_eq!(&output.pixels[4..], &frame(2, 2).pixels[4..]);
        pointer.composite(&mut output, POINT { x: 1, y: 1 });
        assert_eq!(&output.pixels[12..], &[1, 2, 3, 255]);
        let before = output.pixels.clone();
        for position in [POINT { x: i32::MAX, y: i32::MAX }, POINT { x: i32::MIN, y: i32::MIN }] {
            pointer.composite(&mut output, position);
            assert_eq!(output.pixels, before);
        }
    }

    #[test]
    fn transparent_color_preserves_desktop() {
        let pointer = shape(COLOR, 1, 1, 4, vec![255, 100, 200, 0]);
        let mut output = frame(1, 1);
        pointer.composite(&mut output, POINT::default());
        assert_eq!(output.pixels, frame(1, 1).pixels);
    }

    #[test]
    fn rejects_truncated_buffers_and_invalid_mask_layouts() {
        let info = DXGI_OUTDUPL_POINTER_SHAPE_INFO {
            Type: COLOR, Width: 2, Height: 1, Pitch: 8, ..Default::default()
        };
        assert!(PointerShape::new(info, vec![0; 7]).is_err());
        assert!(PointerShape::new(DXGI_OUTDUPL_POINTER_SHAPE_INFO { Pitch: 4, ..info }, vec![0; 8]).is_err());
        assert!(PointerShape::new(DXGI_OUTDUPL_POINTER_SHAPE_INFO { Type: MONOCHROME, ..info }, vec![0; 8]).is_err());
    }
}
