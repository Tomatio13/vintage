//! Read-only file access registered to one Preview workspace.
//! All constructors and I/O methods run on background workers.
use anyhow::{ensure, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const MAX_DIRECTORY_ENTRIES: usize = 2000;
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 16_000_000;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub symlink: bool,
}
#[derive(Clone, Debug)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub truncated: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
}
#[derive(Clone, Debug)]
pub enum Content {
    Text {
        text: String,
        truncated: bool,
        markdown: bool,
    },
    Image {
        bytes: Vec<u8>,
        kind: ImageKind,
    },
    Unsupported(String),
}
#[derive(Clone, Debug)]
pub struct Preview {
    pub size: u64,
    pub content: Content,
}

pub struct WorkspaceFiles {
    id: u64,
    root: PathBuf,
    active: AtomicBool,
}
impl WorkspaceFiles {
    pub fn register(id: u64, root: &Path) -> Result<Self> {
        ensure!(id != 0, "Invalid workspace identifier");
        let root = super::native_sessions::workspace_root(root)
            .context("The workspace folder is unavailable")?;
        Ok(Self {
            id,
            root,
            active: AtomicBool::new(true),
        })
    }
    pub fn revoke(&self) {
        self.active.store(false, Ordering::Release);
    }
    fn authorize(&self, id: u64) -> Result<()> {
        ensure!(
            self.active.load(Ordering::Acquire) && self.id == id,
            "The workspace file session is closed or invalid"
        );
        Ok(())
    }
    fn resolve(&self, id: u64, relative: &str) -> Result<PathBuf> {
        self.authorize(id)?;
        ensure!(relative.len() <= 8192, "The file path is too long");
        ensure!(
            !relative.contains(['\\', ':', '\0']),
            "Use a normalized relative file path"
        );
        if !relative.is_empty() {
            ensure!(
                relative
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
                "Use a normalized relative file path"
            );
        }
        let target = self
            .root
            .join(relative)
            .canonicalize()
            .context("The requested file is unavailable")?;
        ensure!(
            target.starts_with(&self.root),
            "The requested file is outside this workspace"
        );
        Ok(target)
    }
    pub fn list(&self, id: u64, relative: &str) -> Result<Listing> {
        let target = self.resolve(id, relative)?;
        ensure!(target.is_dir(), "The requested path is not a folder");
        let mut entries = Vec::new();
        let mut truncated = false;
        for (index, child) in fs::read_dir(target)
            .context("Cannot read this folder")?
            .enumerate()
        {
            if index == MAX_DIRECTORY_ENTRIES {
                truncated = true;
                break;
            }
            let child = child.context("Cannot read a folder entry")?;
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("A filename is not valid Unicode"))?;
            let kind = child.file_type().context("Cannot inspect a folder entry")?;
            let path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            entries.push(Entry {
                name,
                path,
                directory: kind.is_dir(),
                symlink: kind.is_symlink(),
            });
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        self.authorize(id)?;
        Ok(Listing { entries, truncated })
    }
    pub fn preview(&self, id: u64, relative: &str) -> Result<Preview> {
        ensure!(!relative.is_empty(), "Select a file to preview");
        let target = self.resolve(id, relative)?;
        ensure!(target.is_file(), "Only regular files can be previewed");
        let mut options = OpenOptions::new();
        options.read(true);
        // Avoid blocking on a FIFO or following a replaced final symlink.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
        }
        let file = options.open(&target).context("Cannot open this file")?;
        let metadata = file.metadata().context("Cannot inspect this file")?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Only regular files can be previewed"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "Cannot preview a reparse point"
            );
        }
        let size = metadata.len();
        let extension = Path::new(relative)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let image = matches!(extension.as_str(), "png" | "jpg" | "jpeg");
        let limit = if image {
            MAX_IMAGE_BYTES
        } else {
            MAX_TEXT_BYTES
        };
        if image && size > limit as u64 {
            return Ok(Preview {
                size,
                content: Content::Unsupported("Image preview is limited to 8 MiB".into()),
            });
        }
        let mut bytes = Vec::new();
        file.take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .context("Cannot read this file")?;
        self.authorize(id)?;
        let truncated = bytes.len() > limit;
        bytes.truncate(limit);
        let content = if image {
            if truncated {
                Content::Unsupported("Image preview is limited to 8 MiB".into())
            } else {
                match image_dimensions(&bytes) {
                    Some((kind,width,height)) if width > 0 && height > 0 && width <= 8192 && height <= 8192 && u64::from(width) * u64::from(height) <= MAX_IMAGE_PIXELS => Content::Image { bytes, kind },
                    _ => Content::Unsupported("Unsupported or oversized image (maximum 16 megapixels, 8192 pixels per side)".into()),
                }
            }
        } else if bytes.contains(&0) {
            Content::Unsupported("Binary preview is not available for this file type".into())
        } else {
            let end = match std::str::from_utf8(&bytes) {
                Ok(_) => bytes.len(),
                Err(error) if truncated && error.error_len().is_none() => error.valid_up_to(),
                Err(_) => {
                    return Ok(Preview {
                        size,
                        content: Content::Unsupported(
                            "This file is not UTF-8 text; binary preview is unavailable".into(),
                        ),
                    })
                }
            };
            let text = String::from_utf8(bytes[..end].to_vec()).expect("validated UTF-8");
            Content::Text {
                text,
                truncated,
                markdown: matches!(extension.as_str(), "md" | "markdown" | "mdown"),
            }
        };
        Ok(Preview { size, content })
    }
}

fn image_dimensions(bytes: &[u8]) -> Option<(ImageKind, u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16)? == b"IHDR" {
        return Some((
            ImageKind::Png,
            u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
            u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
        ));
    }
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut offset = 2;
    while offset < bytes.len() {
        if *bytes.get(offset)? != 0xff {
            return None;
        }
        while *bytes.get(offset)? == 0xff {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if matches!(marker, 0xd9 | 0xda) {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes(
            bytes.get(offset..offset + 2)?.try_into().ok()?,
        ));
        if length < 2 || offset.checked_add(length)? > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 8 {
                return None;
            }
            let height = u16::from_be_bytes(bytes.get(offset + 3..offset + 5)?.try_into().ok()?);
            let width = u16::from_be_bytes(bytes.get(offset + 5..offset + 7)?.try_into().ok()?);
            return Some((ImageKind::Jpeg, u32::from(width), u32::from(height)));
        }
        offset += length;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "vintage-files-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn service(&self) -> WorkspaceFiles {
            WorkspaceFiles::register(1, &self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn listing_is_scoped_sorted_and_revocable() {
        let f = Fixture::new();
        fs::write(f.0.join("z.md"), "# synthetic").unwrap();
        fs::create_dir(f.0.join("a")).unwrap();
        let service = f.service();
        let list = service.list(1, "").unwrap();
        assert_eq!(
            list.entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "z.md"]
        );
        assert!(service.list(2, "").is_err());
        for path in [
            "../outside",
            "/etc/passwd",
            "a/../z.md",
            "a//b",
            "a/./b",
            "C:\\outside",
            "\\\\host\\share",
        ] {
            assert!(service.preview(1, path).is_err(), "{path}");
        }
        assert!(matches!(
            service.preview(1, "z.md").unwrap().content,
            Content::Text { markdown: true, .. }
        ));
        service.revoke();
        assert!(service.preview(1, "z.md").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_escape_and_special_files_are_rejected() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("outside"), "synthetic").unwrap();
        symlink(outside.0.join("outside"), f.0.join("escape")).unwrap();
        symlink(&outside.0, f.0.join("outside-dir")).unwrap();
        let service = f.service();
        assert!(service.preview(1, "escape").is_err());
        assert!(service.list(1, "outside-dir").is_err());
        let fifo = std::ffi::CString::new(f.0.join("fifo").as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(service.preview(1, "fifo").is_err());
    }
    #[test]
    fn bounded_text_handles_utf8_boundary_binary_empty_and_changed_files() {
        let f = Fixture::new();
        let service = f.service();
        let mut text = "x".repeat(MAX_TEXT_BYTES - 1);
        text.push_str("日本語");
        fs::write(f.0.join("large.txt"), text).unwrap();
        match service.preview(1, "large.txt").unwrap().content {
            Content::Text {
                text, truncated, ..
            } => {
                assert!(truncated);
                assert_eq!(text.len(), MAX_TEXT_BYTES - 1);
            }
            _ => panic!("text expected"),
        }
        fs::write(f.0.join("binary"), [0, 1, 2]).unwrap();
        assert!(matches!(
            service.preview(1, "binary").unwrap().content,
            Content::Unsupported(_)
        ));
        fs::write(f.0.join("empty"), "").unwrap();
        assert!(
            matches!(service.preview(1,"empty").unwrap().content,Content::Text{text,..} if text.is_empty())
        );
        fs::remove_file(f.0.join("empty")).unwrap();
        assert!(service.preview(1, "empty").is_err());
    }
    #[test]
    fn image_headers_are_bounded_and_malformed_data_is_rejected() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(2u32.to_be_bytes());
        png.extend(3u32.to_be_bytes());
        assert_eq!(image_dimensions(&png), Some((ImageKind::Png, 2, 3)));
        assert!(image_dimensions(&[0xff, 0xd8, 0xff, 0xc0, 0, 1]).is_none());
        let f = Fixture::new();
        png[16..20].copy_from_slice(&100000u32.to_be_bytes());
        fs::write(f.0.join("huge.png"), png).unwrap();
        assert!(matches!(
            f.service().preview(1, "huge.png").unwrap().content,
            Content::Unsupported(_)
        ));
    }
}
