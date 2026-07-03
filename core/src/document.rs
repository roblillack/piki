use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone)]
pub struct Document {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
    pub modified_time: Option<SystemTime>,
}

pub struct DocumentStore {
    base_path: PathBuf,
}

/// Returns true if the name already ends with a (case-insensitive) `.md`
/// extension.
///
/// Unlike `Path::extension`, this treats any other dots in the page name
/// (e.g. "sprint-q2.6") as part of the name rather than a file extension.
pub fn has_md_extension(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 3 && bytes[bytes.len() - 3..].eq_ignore_ascii_case(b".md")
}

/// Append a `.md` extension to a page name unless it already has one.
///
/// This intentionally avoids `Path::set_extension`, which would mistake a dot
/// inside the page name for a file extension (turning "sprint-q2.6" into the
/// extension-less "sprint-q2.6" or, worse, "sprint-q2.md").
pub fn ensure_md_extension(name: &str) -> String {
    if has_md_extension(name) {
        name.to_string()
    } else {
        format!("{name}.md")
    }
}

/// Name of the per-folder subdirectory that holds a page's attachments.
///
/// Chosen to match Logseq's `assets/` convention and Obsidian's "subfolder
/// under current folder" attachment setting, so vaults stay portable between
/// tools. See `specs/image-support.md`.
pub const ATTACHMENTS_DIR: &str = "assets";

/// Sanitise a dropped file's name into one that is safe both as an on-disk
/// filename and as a Markdown link destination.
///
/// Markdown link destinations can't contain unescaped spaces or brackets, so
/// rather than percent-encode (which would then need decoding everywhere and
/// is fragile across the editor's Markdown round-trip) we replace any run of
/// problematic characters with a single dash. Unicode letters/digits, dots,
/// dashes and underscores are preserved.
pub fn sanitize_attachment_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for c in name.chars() {
        if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
            // Avoid leaving a dash stranded right before the extension dot
            // (e.g. "photo (1).png" -> "photo-1.png" rather than "photo-1-.png").
            if c == '.' && out.ends_with('-') {
                out.pop();
            }
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "attachment".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Split a sanitised filename into its stem and extension (without the dot).
/// Used to insert a numeric suffix before the extension on name collisions.
fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    }
}

impl DocumentStore {
    pub fn new(base_path: PathBuf) -> Self {
        DocumentStore { base_path }
    }

    /// Load a document by name (with or without .md extension)
    /// If the file doesn't exist, creates an empty document that will be saved on first write
    pub fn load(&self, name: &str) -> Result<Document, String> {
        // Append `.md` unless the name already carries it. Note we deliberately
        // do not rely on `Path::extension`, which would treat the trailing part
        // of a dotted page name (e.g. "sprint-q2.6") as the extension and skip
        // adding `.md`.
        let path = self.base_path.join(ensure_md_extension(name));

        // Read file content and metadata if it exists, otherwise create empty document
        let (content, modified_time) = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read '{}': {}", name, e))?;

            // Get modification time
            let mtime = fs::metadata(&path).ok().and_then(|m| m.modified().ok());

            (content, mtime)
        } else {
            (String::new(), None)
        };

        Ok(Document {
            name: name.to_string(),
            path,
            content,
            modified_time,
        })
    }

    /// Recursively list all markdown files in the directory and subdirectories
    /// Returns relative paths from base_path (e.g., "project-a/standup")
    pub fn list_all_documents(&self) -> Result<Vec<String>, String> {
        let mut docs = Vec::new();
        Self::walk_directory(&self.base_path, "", &mut docs)?;
        Ok(docs)
    }

    /// Helper function to recursively walk directories
    fn walk_directory(dir: &PathBuf, prefix: &str, docs: &mut Vec<String>) -> Result<(), String> {
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory '{}': {}", dir.display(), e))?;

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let full_name = if prefix.is_empty() {
                        name.to_string()
                    } else {
                        format!("{}/{}", prefix, name)
                    };
                    docs.push(full_name);
                }
            } else if path.is_dir() {
                // Recursively walk subdirectories
                if let Some(dir_name) = path.file_name().and_then(|s| s.to_str()) {
                    let new_prefix = if prefix.is_empty() {
                        dir_name.to_string()
                    } else {
                        format!("{}/{}", prefix, dir_name)
                    };
                    Self::walk_directory(&path, &new_prefix, docs)?;
                }
            }
        }

        Ok(())
    }

    /// Save document content
    /// Creates parent directories if they don't exist
    pub fn save(&self, doc: &Document) -> Result<(), String> {
        // Create parent directories if they don't exist
        if let Some(parent) = doc.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories for '{}': {}", doc.name, e))?;
        }

        fs::write(&doc.path, &doc.content)
            .map_err(|e| format!("Failed to save '{}': {}", doc.name, e))
    }

    /// The directory that holds `page_name`'s Markdown file. Attachments live in
    /// a sibling `assets/` folder, so this is resolved relative to it.
    fn page_dir(&self, page_name: &str) -> PathBuf {
        let page_path = self.base_path.join(ensure_md_extension(page_name));
        page_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.base_path.clone())
    }

    /// The `assets/` folder for a page (per-folder convention): a sibling of the
    /// page's Markdown file. Not guaranteed to exist yet.
    pub fn attachments_dir(&self, page_name: &str) -> PathBuf {
        self.page_dir(page_name).join(ATTACHMENTS_DIR)
    }

    /// Copy an external file into `page_name`'s `assets/` folder and return the
    /// Markdown-relative link destination to embed (e.g. `assets/photo.png`).
    ///
    /// The filename is sanitised (see [`sanitize_attachment_name`]). On a name
    /// collision an identical existing file is reused; otherwise a `-N` suffix
    /// is added so distinct files never clobber each other.
    pub fn add_attachment(&self, page_name: &str, source: &Path) -> Result<String, String> {
        let dir = self.attachments_dir(page_name);
        fs::create_dir_all(&dir).map_err(|e| {
            format!(
                "Failed to create attachments directory '{}': {}",
                dir.display(),
                e
            )
        })?;

        let raw_name = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("Invalid attachment filename: {}", source.display()))?;
        let safe_name = sanitize_attachment_name(raw_name);

        let bytes = fs::read(source)
            .map_err(|e| format!("Failed to read '{}': {}", source.display(), e))?;

        let final_name = self.unique_attachment_name(&dir, &safe_name, &bytes);
        let target = dir.join(&final_name);
        // `unique_attachment_name` only returns the name of an already-present
        // file when its bytes are identical, so writing again is redundant.
        if !target.exists() {
            fs::write(&target, &bytes)
                .map_err(|e| format!("Failed to write '{}': {}", target.display(), e))?;
        }

        Ok(format!("{ATTACHMENTS_DIR}/{final_name}"))
    }

    /// Pick a non-colliding filename in `dir`. Reuses an existing file whose
    /// bytes match `bytes` (so re-dropping the same file doesn't duplicate it),
    /// otherwise appends `-1`, `-2`, … before the extension.
    fn unique_attachment_name(&self, dir: &Path, name: &str, bytes: &[u8]) -> String {
        let (stem, ext) = split_name(name);
        let mut candidate = name.to_string();
        let mut n = 1;
        loop {
            let path = dir.join(&candidate);
            if !path.exists() {
                return candidate;
            }
            if fs::read(&path).map(|b| b == bytes).unwrap_or(false) {
                return candidate; // identical file already present – reuse it
            }
            candidate = match ext {
                Some(ext) => format!("{stem}-{n}.{ext}"),
                None => format!("{stem}-{n}"),
            };
            n += 1;
        }
    }

    /// Resolve a link destination that may point at a local attachment, relative
    /// to `page_name`'s folder. Returns the absolute path only when it is an
    /// existing, non-Markdown file — Markdown files are always pages, never
    /// attachments (see `specs/image-support.md`), so they resolve to `None`
    /// and fall through to normal wiki navigation.
    pub fn resolve_attachment(&self, page_name: &str, dest: &str) -> Option<PathBuf> {
        let dest = dest.trim();
        if dest.is_empty() || has_md_extension(dest) {
            return None;
        }
        let candidate = self.page_dir(page_name).join(dest);
        candidate.is_file().then_some(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_load_existing_file() {
        let store = DocumentStore::new("example-wiki".into());
        let doc = store.load("frontpage").unwrap();
        assert!(!doc.content.is_empty());
        assert_eq!(doc.name, "frontpage");
    }

    #[test]
    fn test_load_non_existent_file() {
        let temp_dir = env::temp_dir().join("piki-test-load");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let store = DocumentStore::new(temp_dir.clone());
        let doc = store.load("non-existent").unwrap();

        assert_eq!(doc.content, "");
        assert_eq!(doc.name, "non-existent");
        assert_eq!(doc.path, temp_dir.join("non-existent.md"));

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_dotted_name_gets_md_extension() {
        let temp_dir = env::temp_dir().join("piki-test-dotted");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let store = DocumentStore::new(temp_dir.clone());

        // A page name with a dot (e.g. "sprint-q2.6") must still get `.md`
        // appended rather than treating ".6" as the extension.
        fs::write(temp_dir.join("sprint-q2.6.md"), "hello").unwrap();
        let doc = store.load("sprint-q2.6").unwrap();

        assert_eq!(doc.path, temp_dir.join("sprint-q2.6.md"));
        assert_eq!(doc.content, "hello");
        assert_eq!(doc.name, "sprint-q2.6");

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_name_with_md_extension_not_doubled() {
        let temp_dir = env::temp_dir().join("piki-test-md-suffix");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let store = DocumentStore::new(temp_dir.clone());
        let doc = store.load("notes.md").unwrap();

        assert_eq!(doc.path, temp_dir.join("notes.md"));

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_md_extension_helpers() {
        assert!(has_md_extension("notes.md"));
        assert!(has_md_extension("notes.MD"));
        assert!(!has_md_extension("sprint-q2.6"));
        assert!(!has_md_extension("md"));

        assert_eq!(ensure_md_extension("sprint-q2.6"), "sprint-q2.6.md");
        assert_eq!(ensure_md_extension("notes.md"), "notes.md");
        assert_eq!(ensure_md_extension("notes.MD"), "notes.MD");
    }

    #[test]
    fn test_load_nested_path() {
        let temp_dir = env::temp_dir().join("piki-test-nested");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let store = DocumentStore::new(temp_dir.clone());
        let doc = store.load("project-a/standup").unwrap();

        assert_eq!(doc.content, "");
        assert_eq!(doc.name, "project-a/standup");
        assert_eq!(doc.path, temp_dir.join("project-a/standup.md"));

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_save_creates_parent_directories() {
        let temp_dir = env::temp_dir().join("piki-test-save");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let store = DocumentStore::new(temp_dir.clone());
        let mut doc = store.load("nested/dir/page").unwrap();
        doc.content = "Test content".to_string();

        store.save(&doc).unwrap();

        // Verify file was created
        assert!(doc.path.exists());
        assert_eq!(fs::read_to_string(&doc.path).unwrap(), "Test content");

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_sanitize_attachment_name() {
        assert_eq!(sanitize_attachment_name("My Report.pdf"), "My-Report.pdf");
        assert_eq!(sanitize_attachment_name("photo (1).png"), "photo-1.png");
        assert_eq!(sanitize_attachment_name("a  b   c.txt"), "a-b-c.txt");
        assert_eq!(sanitize_attachment_name("clean_name-1.PNG"), "clean_name-1.PNG");
        // Nothing usable left -> fallback name.
        assert_eq!(sanitize_attachment_name("***"), "attachment");
        // Unicode letters are preserved.
        assert_eq!(sanitize_attachment_name("café.png"), "café.png");
    }

    #[test]
    fn test_attachments_dir_is_page_sibling() {
        let store = DocumentStore::new(PathBuf::from("/vault"));
        assert_eq!(store.attachments_dir("frontpage"), PathBuf::from("/vault/assets"));
        assert_eq!(
            store.attachments_dir("work/projects/q4"),
            PathBuf::from("/vault/work/projects/assets")
        );
    }

    #[test]
    fn test_add_attachment_copies_and_returns_relative_link() {
        let temp_dir = env::temp_dir().join("piki-test-attach");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // A source file to attach, living outside the vault.
        let src = temp_dir.join("source.png");
        fs::write(&src, b"image-bytes").unwrap();

        let store = DocumentStore::new(temp_dir.join("vault"));
        let link = store.add_attachment("notes", &src).unwrap();

        assert_eq!(link, "assets/source.png");
        let copied = temp_dir.join("vault/assets/source.png");
        assert!(copied.is_file());
        assert_eq!(fs::read(&copied).unwrap(), b"image-bytes");

        // Re-dropping the identical file reuses it (no duplicate).
        let link2 = store.add_attachment("notes", &src).unwrap();
        assert_eq!(link2, "assets/source.png");

        // A different file with the same name gets a numeric suffix.
        let src2 = temp_dir.join("other/source.png");
        fs::create_dir_all(src2.parent().unwrap()).unwrap();
        fs::write(&src2, b"different-bytes").unwrap();
        let link3 = store.add_attachment("notes", &src2).unwrap();
        assert_eq!(link3, "assets/source-1.png");

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_resolve_attachment() {
        let temp_dir = env::temp_dir().join("piki-test-resolve");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("work/assets")).unwrap();
        fs::write(temp_dir.join("work/assets/report.pdf"), b"pdf").unwrap();
        fs::write(temp_dir.join("work/other.md"), b"# page").unwrap();

        let store = DocumentStore::new(temp_dir.clone());

        // An existing non-Markdown file resolves relative to the page's folder.
        assert_eq!(
            store.resolve_attachment("work/q4", "assets/report.pdf"),
            Some(temp_dir.join("work/assets/report.pdf"))
        );
        // Markdown links are pages, not attachments.
        assert_eq!(store.resolve_attachment("work/q4", "other.md"), None);
        // Missing files and plain page names fall through to wiki navigation.
        assert_eq!(store.resolve_attachment("work/q4", "assets/missing.png"), None);
        assert_eq!(store.resolve_attachment("work/q4", "some-page"), None);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_list_all_documents_recursive() {
        let temp_dir = env::temp_dir().join("piki-test-list-all");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let store = DocumentStore::new(temp_dir.clone());

        // Create some test files
        fs::write(temp_dir.join("root.md"), "root").unwrap();
        fs::create_dir_all(temp_dir.join("dir1")).unwrap();
        fs::write(temp_dir.join("dir1/page1.md"), "page1").unwrap();
        fs::create_dir_all(temp_dir.join("dir1/subdir")).unwrap();
        fs::write(temp_dir.join("dir1/subdir/page2.md"), "page2").unwrap();

        let docs = store.list_all_documents().unwrap();

        assert!(docs.contains(&"root".to_string()));
        assert!(docs.contains(&"dir1/page1".to_string()));
        assert!(docs.contains(&"dir1/subdir/page2".to_string()));
        assert_eq!(docs.len(), 3);

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }
}
