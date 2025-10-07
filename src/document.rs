use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Document {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
}

pub struct DocumentStore {
    base_path: PathBuf,
}

impl DocumentStore {
    pub fn new(base_path: PathBuf) -> Self {
        DocumentStore { base_path }
    }

    /// Load a document by name (with or without .md extension)
    pub fn load(&self, name: &str) -> Result<Document, String> {
        let mut path = self.base_path.join(name);

        // Try with .md extension if not already present
        if path.extension().is_none() {
            path.set_extension("md");
        }

        // Check if file exists
        if !path.exists() {
            return Err(format!("Document '{}' not found", name));
        }

        // Read file content
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read '{}': {}", name, e))?;

        Ok(Document {
            name: name.to_string(),
            path,
            content,
        })
    }

    /// List all markdown files in the directory
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

    /// Save document content
    pub fn save(&self, doc: &Document) -> Result<(), String> {
        fs::write(&doc.path, &doc.content)
            .map_err(|e| format!("Failed to save '{}': {}", doc.name, e))
    }
}
