//! Output > Snapshot: the composition's picture saved as a PNG in a folder of the user's.

use std::path::{Path, PathBuf};

/// The folder snapshots go to: the one chosen in the preferences, else `Flux Snapshots` in the
/// user's pictures, else a `Snapshots` folder in the app's data.
pub fn folder(chosen: Option<&str>, pictures: Option<PathBuf>, data: &Path) -> PathBuf {
    match chosen.map(str::trim).filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => pictures.map_or_else(|| data.join("Snapshots"), |dir| dir.join("Flux Snapshots")),
    }
}

/// The path of a snapshot taken at `stamp` (`2026-01-31 12-00-00`) in `dir` that no file has yet:
/// ` 2`, ` 3`... follow the stamp when two are taken in one second.
pub fn file(dir: &Path, stamp: &str) -> PathBuf {
    let first = dir.join(format!("Flux {stamp}.png"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("Flux {stamp} {n}.png")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

/// `rgba` (`width * height` pixels, rows top to bottom) as a PNG.
pub fn encode(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err("the picture does not have the size it says".into());
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(rgba)
        .map_err(|err| err.to_string())?;
    writer.finish().map_err(|err| err.to_string())?;
    Ok(out)
}

/// Saves `rgba` as a PNG in `dir`, which is made if need be, and answers where.
pub fn save(
    dir: &Path,
    stamp: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<PathBuf, String> {
    let png = encode(rgba, width, height)?;
    std::fs::create_dir_all(dir)
        .map_err(|err| format!("could not make the folder {}: {err}", dir.display()))?;
    let path = file(dir, stamp);
    std::fs::write(&path, png)
        .map_err(|err| format!("could not save {}: {err}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_should_make_a_png_of_the_size_given() {
        let rgba = vec![200; 6 * 4 * 4];
        let png = encode(&rgba, 6, 4).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!((info.width, info.height), (6, 4));
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buffer).unwrap();
        assert_eq!(buffer, rgba);
    }

    #[test]
    fn encode_should_refuse_a_picture_of_another_size() {
        assert!(encode(&[0; 10], 6, 4).is_err());
    }

    #[test]
    fn folder_should_prefer_the_choice_then_pictures_then_the_app_data() {
        let data = Path::new("/data");
        let pictures = Some(PathBuf::from("/pics"));
        assert_eq!(
            folder(Some("/mine"), pictures.clone(), data),
            Path::new("/mine")
        );
        assert_eq!(
            folder(Some("  "), pictures.clone(), data),
            Path::new("/pics/Flux Snapshots")
        );
        assert_eq!(folder(None, None, data), Path::new("/data/Snapshots"));
    }

    #[test]
    fn save_should_make_the_folder_and_not_overwrite_a_snapshot_of_the_same_second() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a").join("b");
        let rgba = vec![0; 4];
        let one = save(&dir, "2026-01-31 12-00-00", &rgba, 1, 1).unwrap();
        let two = save(&dir, "2026-01-31 12-00-00", &rgba, 1, 1).unwrap();
        assert_ne!(one, two);
        assert!(one.ends_with("Flux 2026-01-31 12-00-00.png") && two.exists());
    }
}
