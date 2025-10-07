use std::fs;
use std::path::PathBuf;
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

impl DocumentStore {
    pub fn new(base_path: PathBuf) -> Self {
        DocumentStore { base_path }
    }

    /// Load a document by name (with or without .md extension)
    /// If the file doesn't exist, creates an empty document that will be saved on first write
    pub fn load(&self, name: &str) -> Result<Document, String> {
        let mut path = self.base_path.join(name);

        // Try with .md extension if not already present
        if path.extension().is_none() {
            path.set_extension("md");
        }

        // Read file content and metadata if it exists, otherwise create empty document
        let (content, modified_time) = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read '{}': {}", name, e))?;

            // Get modification time
            let mtime = fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok());

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

    /// List all markdown files in the directory (non-recursive, immediate children only)
    pub fn list_documents(&self) -> Result<Vec<String>, String> {
        let mut docs = Vec::new();

        let entries = fs::read_dir(&self.base_path)
            .map_err(|e| format!("Failed to read directory: {}", e))?;

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        docs.push(name.to_string());
                    }
                }
            }
        }

        docs.sort();
        Ok(docs)
    }

    /// Recursively list all markdown files in the directory and subdirectories
    /// Returns relative paths from base_path (e.g., "project-a/standup")
    pub fn list_all_documents(&self) -> Result<Vec<String>, String> {
        let mut docs = Vec::new();
        self.walk_directory(&self.base_path, "", &mut docs)?;
        Ok(docs)
    }

    /// Helper function to recursively walk directories
    fn walk_directory(&self, dir: &PathBuf, prefix: &str, docs: &mut Vec<String>) -> Result<(), String> {
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory '{}': {}", dir.display(), e))?;

        for entry in entries {
            if let Ok(entry) = entry {
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
                        self.walk_directory(&path, &new_prefix, docs)?;
                    }
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
        let temp_dir = env::temp_dir().join("fliki-test-load");
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
    fn test_load_nested_path() {
        let temp_dir = env::temp_dir().join("fliki-test-nested");
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
        let temp_dir = env::temp_dir().join("fliki-test-save");
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
    fn test_list_all_documents_recursive() {
        let temp_dir = env::temp_dir().join("fliki-test-list-all");
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
