use crate::Options;
use anyhow::{Context, Result, bail};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use zenkit::vfs::{ReadOptions, Vdf};

const MAX_FILE: usize = 512 * 1024 * 1024;

enum Source {
    Archive(usize, usize),
    Loose(PathBuf),
}

pub struct Assets {
    archives: Vec<Vdf>,
    files: HashMap<Vec<u8>, Source>,
}

pub fn key(name: &[u8]) -> Vec<u8> {
    name.iter()
        .map(|&c| {
            if c == b'\\' {
                b'/'
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect()
}

pub fn basename(name: &[u8]) -> &[u8] {
    name.rsplit(|c| *c == b'/' || *c == b'\\')
        .next()
        .unwrap_or(name)
}

pub fn with_extension(name: &[u8], extension: &[u8]) -> Vec<u8> {
    let name = basename(name);
    let stem = name
        .iter()
        .rposition(|c| *c == b'.')
        .map_or(name, |i| &name[..i]);
    [stem, b".", extension].concat()
}

impl Assets {
    pub fn load(options: &Options) -> Result<Self> {
        let mut assets = Self {
            archives: vec![],
            files: HashMap::new(),
        };
        if let Some(root) = &options.game_dir {
            let data = root.join("Data");
            let mut archives = vec![];
            for entry in
                fs::read_dir(&data).with_context(|| format!("opening {}", data.display()))?
            {
                let path = entry?.path();
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase();
                if name.ends_with(".vdf")
                    && ["worlds", "meshes", "textures", "anims"]
                        .iter()
                        .any(|p| name.starts_with(p))
                {
                    archives.push(path);
                }
            }
            archives.sort_by_key(|p| p.file_name().unwrap_or_default().to_ascii_lowercase());
            if archives.is_empty() {
                bail!(
                    "no world/mesh/texture/animation VDF archives in {}",
                    data.display()
                );
            }
            for path in archives {
                assets.mount(&path)?;
            }
        }
        for path in &options.mounts {
            assets.mount(path)?;
        }
        eprintln!(
            "Indexed {} asset names in {} archives",
            assets.files.len(),
            assets.archives.len()
        );
        Ok(assets)
    }

    fn mount(&mut self, path: &Path) -> Result<()> {
        eprintln!("Mounting {}", path.display());
        if path.is_dir() {
            let mut pending = vec![path.to_owned()];
            let mut count = 0;
            while let Some(dir) = pending.pop() {
                let mut entries = fs::read_dir(&dir)?.collect::<std::io::Result<Vec<_>>>()?;
                entries.sort_by_key(|e| e.file_name());
                for entry in entries {
                    count += 1;
                    if count > 200_000 {
                        bail!("loose asset directory exceeds 200000 entries");
                    }
                    let ty = entry.file_type()?;
                    if ty.is_dir() {
                        pending.push(entry.path());
                    } else if ty.is_file() {
                        let full = entry.path();
                        let relative = full.strip_prefix(path)?.to_string_lossy();
                        self.files
                            .insert(key(relative.as_bytes()), Source::Loose(full.clone()));
                        self.files.insert(
                            key(entry.file_name().to_string_lossy().as_bytes()),
                            Source::Loose(full),
                        );
                    }
                }
            }
        } else {
            let limits = zenkit::Limits {
                input_bytes: 2 * 1024 * 1024 * 1024,
                payload_bytes: 4 * 1024 * 1024 * 1024,
                file_payload_bytes: MAX_FILE,
                compression_input_bytes: 4 * 1024 * 1024 * 1024,
                ..Default::default()
            };
            let archive = Vdf::from_path(path, ReadOptions { limits })
                .with_context(|| format!("mounting {}", path.display()))?;
            let archive_id = self.archives.len();
            for (index, entry) in archive.entries().iter().enumerate() {
                if entry.is_directory() {
                    continue;
                }
                let mut parts = vec![entry.name()];
                let mut parent = entry.parent();
                while let Some(i) = parent {
                    parts.push(archive.entries()[i].name());
                    parent = archive.entries()[i].parent();
                }
                parts.reverse();
                self.files
                    .insert(key(&parts.join(&b'/')), Source::Archive(archive_id, index));
                self.files
                    .insert(key(entry.name()), Source::Archive(archive_id, index));
            }
            self.archives.push(archive);
        }
        Ok(())
    }

    pub fn read(&self, name: &[u8]) -> Result<Option<Vec<u8>>> {
        let normalized = key(name);
        let Some(source) = self
            .files
            .get(&normalized)
            .or_else(|| self.files.get(&key(basename(name))))
        else {
            return Ok(None);
        };
        Ok(Some(match source {
            Source::Archive(archive, entry) => self.archives[*archive]
                .payload(*entry)
                .context("indexed asset has no payload")?
                .to_vec(),
            Source::Loose(path) => zenkit::io::read_bounded(fs::File::open(path)?, MAX_FILE)
                .with_context(|| format!("reading {}", path.display()))?,
        }))
    }

    pub fn world(&self, name: &str) -> Result<Vec<u8>> {
        let path = Path::new(name);
        if path.is_file() {
            Ok(zenkit::io::read_bounded(fs::File::open(path)?, MAX_FILE)?)
        } else {
            self.read(name.as_bytes())?
                .with_context(|| format!("world {name:?} not found; pass --game-dir or --mount"))
        }
    }

    pub fn texture(&self, name: &[u8]) -> Result<Option<image::RgbaImage>> {
        if name.is_empty() {
            return Ok(None);
        }
        let tex = with_extension(name, b"TEX");
        let compiled = [tex.strip_suffix(b".TEX").unwrap(), b"-C.TEX"].concat();
        for candidate in [&compiled, &tex] {
            if let Some(bytes) = self.read(candidate)? {
                let limits = zenkit::texture::Limits::default();
                let texture = zenkit::texture::Texture::from_bytes(&bytes, limits)?;
                // Keep texture residency bounded for this simple viewer.
                let mip = texture
                    .mips()
                    .find(|m| m.width() <= 1024 && m.height() <= 1024)
                    .or_else(|| texture.mip(0))
                    .context("texture has no mipmaps")?;
                return Ok(Some(
                    image::RgbaImage::from_raw(mip.width(), mip.height(), mip.rgba8(limits)?)
                        .context("invalid texture dimensions")?,
                ));
            }
        }
        if let Some(bytes) = self.read(name)? {
            let image = if key(name).ends_with(b".TGA") {
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)?
            } else {
                image::load_from_memory(&bytes)?
            };
            return Ok(Some(image.into_rgba8()));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gothic_asset_names_are_case_insensitive_and_use_compiled_basenames() {
        assert_eq!(key(b"_work\\Data/rock.3ds"), b"_WORK/DATA/ROCK.3DS");
        assert_eq!(with_extension(b"Meshes\\Rock.3DS", b"MRM"), b"Rock.MRM");
    }
}
