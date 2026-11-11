use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn halve(&self) -> Frame {
        let width = self.width / 2;
        let height = self.height / 2;
        let mut bgra = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                for channel in 0..4 {
                    let at = |dx: usize, dy: usize| {
                        u32::from(self.bgra[((y * 2 + dy) * self.width + x * 2 + dx) * 4 + channel])
                    };
                    bgra.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
                }
            }
        }
        Frame {
            width,
            height,
            bgra,
        }
    }

    pub fn rgb(&self) -> Vec<u8> {
        self.bgra
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect()
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
        let mut encoder =
            png::Encoder::new(BufWriter::new(file), self.width as u32, self.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        let rgba: Vec<u8> = self
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect();
        writer.write_image_data(&rgba).map_err(|e| e.to_string())
    }

    pub fn load_png(path: &Path) -> Result<Frame, String> {
        let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let mut reader = png::Decoder::new(BufReader::new(file))
            .read_info()
            .map_err(|e| e.to_string())?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| "the image is too large".to_string())?;
        let mut data = vec![0; size];
        let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
        if info.bit_depth != png::BitDepth::Eight {
            return Err(format!("bit depth {:?} is not 8", info.bit_depth));
        }
        let pixels = info.width as usize * info.height as usize;
        let bgra: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => data[..pixels * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect(),
            png::ColorType::Rgb => data[..pixels * 3]
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], 255])
                .collect(),
            other => return Err(format!("colour type {other:?} is not RGB or RGBA")),
        };
        Ok(Frame {
            width: info.width as usize,
            height: info.height as usize,
            bgra,
        })
    }
}
