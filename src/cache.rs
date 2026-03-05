use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::CacheError;
use crate::graph::TaskId;

/// Directory name for the cache within workspace root.
const CACHE_DIR: &str = ".guild/cache";

/// A cache entry stored for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Hash of the task inputs.
    pub input_hash: String,
    /// When the cache entry was created.
    pub timestamp: DateTime<Utc>,
    /// Whether the task succeeded.
    pub success: bool,
    /// The command that was executed.
    pub command: String,
}

/// Statistics about the cache.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total number of cache entries.
    pub entry_count: usize,
    /// Total size of the cache directory in bytes.
    pub total_size: u64,
    /// Number of cache hits during this run.
    pub hits: usize,
    /// Number of cache misses during this run.
    pub misses: usize,
}

/// Input-based cache for task outputs.
#[derive(Debug)]
pub struct Cache {
    /// Root directory of the cache (workspace_root/.guild/cache).
    cache_dir: PathBuf,
    /// Statistics for the current run.
    stats: CacheStats,
}

impl Cache {
    /// Create a new cache at the given workspace root.
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            cache_dir: workspace_root.join(CACHE_DIR),
            stats: CacheStats::default(),
        }
    }

    /// Get the path to a cache entry file for a task.
    fn entry_path(&self, task_id: &TaskId) -> PathBuf {
        let key = format!("{}:{}", task_id.project(), task_id.target());
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        self.cache_dir.join(format!("{hash}.json"))
    }

    /// Compute the input hash for a task.
    ///
    /// The hash is computed from:
    /// - The target command
    /// - Sorted input file contents (matched by glob patterns)
    /// - Dependency cache keys (for transitivity)
    pub fn compute_input_hash(
        &self,
        command: &str,
        project_root: &Path,
        input_patterns: &[String],
        dependency_hashes: &[String],
    ) -> Result<String, CacheError> {
        let mut hasher = Sha256::new();

        // Hash the command
        hasher.update(command.as_bytes());
        hasher.update(b"\0");

        // Hash input files sorted by path
        let mut file_hashes: BTreeMap<PathBuf, String> = BTreeMap::new();

        for pattern in input_patterns {
            let full_pattern = project_root.join(pattern);
            let pattern_str = full_pattern.to_string_lossy();

            let entries = glob::glob(&pattern_str).map_err(|e| CacheError::GlobPattern {
                pattern: pattern.clone(),
                source: e,
            })?;

            for entry in entries {
                let path = entry.map_err(|e| CacheError::GlobEntry { source: e })?;
                if path.is_file() {
                    let content = fs::read(&path).map_err(|e| CacheError::ReadFile {
                        path: path.clone(),
                        source: e,
                    })?;
                    let mut file_hasher = Sha256::new();
                    file_hasher.update(&content);
                    let file_hash = format!("{:x}", file_hasher.finalize());
                    file_hashes.insert(path, file_hash);
                }
            }
        }

        // If no input patterns specified, use a default hash based on command only
        // This allows caching for targets without explicit inputs
        for (path, hash) in &file_hashes {
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(b":");
            hasher.update(hash.as_bytes());
            hasher.update(b"\0");
        }

        // Hash dependency cache keys (sorted for determinism)
        let mut sorted_deps: Vec<&String> = dependency_hashes.iter().collect();
        sorted_deps.sort();
        for dep_hash in sorted_deps {
            hasher.update(dep_hash.as_bytes());
            hasher.update(b"\0");
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Check if a task has a valid cache entry.
    ///
    /// Returns `Some(entry)` if the cache is valid, `None` otherwise.
    pub fn check(&mut self, task_id: &TaskId, current_hash: &str) -> Option<CacheEntry> {
        let path = self.entry_path(task_id);

        if !path.exists() {
            self.stats.misses += 1;
            return None;
        }

        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<CacheEntry>(&content) {
                Ok(entry) if entry.input_hash == current_hash && entry.success => {
                    self.stats.hits += 1;
                    Some(entry)
                }
                _ => {
                    self.stats.misses += 1;
                    None
                }
            },
            Err(_) => {
                self.stats.misses += 1;
                None
            }
        }
    }

    /// Write a cache entry for a task.
    pub fn write(
        &self,
        task_id: &TaskId,
        input_hash: String,
        success: bool,
        command: String,
    ) -> Result<(), CacheError> {
        // Ensure cache directory exists
        fs::create_dir_all(&self.cache_dir).map_err(|e| CacheError::CreateDir {
            path: self.cache_dir.clone(),
            source: e,
        })?;

        let entry = CacheEntry {
            input_hash,
            timestamp: Utc::now(),
            success,
            command,
        };

        let path = self.entry_path(task_id);
        let content =
            serde_json::to_string_pretty(&entry).map_err(|e| CacheError::SerializeEntry {
                task: task_id.to_string(),
                source: e,
            })?;

        fs::write(&path, content).map_err(|e| CacheError::WriteFile { path, source: e })?;

        Ok(())
    }

    /// Get cache statistics.
    pub fn stats(&self) -> Result<CacheStats, CacheError> {
        let mut stats = self.stats.clone();

        if self.cache_dir.exists() {
            for entry in fs::read_dir(&self.cache_dir).map_err(|e| CacheError::ReadDir {
                path: self.cache_dir.clone(),
                source: e,
            })? {
                let entry = entry.map_err(|e| CacheError::ReadDir {
                    path: self.cache_dir.clone(),
                    source: e,
                })?;

                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    stats.entry_count += 1;
                    if let Ok(metadata) = fs::metadata(&path) {
                        stats.total_size += metadata.len();
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Clean the cache directory.
    pub fn clean(&self) -> Result<usize, CacheError> {
        if !self.cache_dir.exists() {
            return Ok(0);
        }

        let mut removed = 0;

        for entry in fs::read_dir(&self.cache_dir).map_err(|e| CacheError::ReadDir {
            path: self.cache_dir.clone(),
            source: e,
        })? {
            let entry = entry.map_err(|e| CacheError::ReadDir {
                path: self.cache_dir.clone(),
                source: e,
            })?;

            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                fs::remove_file(&path).map_err(|e| CacheError::RemoveFile {
                    path: path.clone(),
                    source: e,
                })?;
                removed += 1;
            }
        }

        // Try to remove the cache directory if empty
        // Ignore errors since .guild might have other contents
        let _ = fs::remove_dir(&self.cache_dir);
        let _ = fs::remove_dir(self.cache_dir.parent().unwrap_or(&self.cache_dir));

        Ok(removed)
    }

    /// Get the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProjectName, TargetName};
    use tempfile::TempDir;

    fn task_id(project: &str, target: &str) -> TaskId {
        TaskId::new(
            project.parse::<ProjectName>().unwrap(),
            target.parse::<TargetName>().unwrap(),
        )
    }

    #[test]
    fn test_compute_input_hash_deterministic() {
        let temp = TempDir::new().unwrap();
        let cache = Cache::new(temp.path());

        // Create a test file
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

        let hash1 = cache
            .compute_input_hash(
                "cargo build",
                temp.path(),
                &["src/**/*.rs".to_string()],
                &[],
            )
            .unwrap();

        let hash2 = cache
            .compute_input_hash(
                "cargo build",
                temp.path(),
                &["src/**/*.rs".to_string()],
                &[],
            )
            .unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_compute_input_hash_changes_with_file_content() {
        let temp = TempDir::new().unwrap();
        let cache = Cache::new(temp.path());

        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

        let hash1 = cache
            .compute_input_hash(
                "cargo build",
                temp.path(),
                &["src/**/*.rs".to_string()],
                &[],
            )
            .unwrap();

        // Modify the file
        fs::write(src_dir.join("main.rs"), "fn main() { println!(\"hi\"); }").unwrap();

        let hash2 = cache
            .compute_input_hash(
                "cargo build",
                temp.path(),
                &["src/**/*.rs".to_string()],
                &[],
            )
            .unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_input_hash_changes_with_command() {
        let temp = TempDir::new().unwrap();
        let cache = Cache::new(temp.path());

        let hash1 = cache
            .compute_input_hash("cargo build", temp.path(), &[], &[])
            .unwrap();

        let hash2 = cache
            .compute_input_hash("cargo build --release", temp.path(), &[], &[])
            .unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_cache_write_and_check() {
        let temp = TempDir::new().unwrap();
        let mut cache = Cache::new(temp.path());

        let task = task_id("my-app", "build");
        let hash = "abc123".to_string();

        // Initially no cache entry
        assert!(cache.check(&task, &hash).is_none());

        // Write cache entry
        cache
            .write(&task, hash.clone(), true, "cargo build".to_string())
            .unwrap();

        // Now cache hit
        let entry = cache.check(&task, &hash).unwrap();
        assert!(entry.success);
        assert_eq!(entry.input_hash, hash);
    }

    #[test]
    fn test_cache_miss_on_different_hash() {
        let temp = TempDir::new().unwrap();
        let mut cache = Cache::new(temp.path());

        let task = task_id("my-app", "build");

        cache
            .write(&task, "hash1".to_string(), true, "cargo build".to_string())
            .unwrap();

        // Check with different hash
        assert!(cache.check(&task, "hash2").is_none());
    }

    #[test]
    fn test_cache_miss_on_failed_entry() {
        let temp = TempDir::new().unwrap();
        let mut cache = Cache::new(temp.path());

        let task = task_id("my-app", "build");
        let hash = "abc123".to_string();

        // Write failed entry
        cache
            .write(&task, hash.clone(), false, "cargo build".to_string())
            .unwrap();

        // Failed entries don't count as cache hits
        assert!(cache.check(&task, &hash).is_none());
    }

    #[test]
    fn test_cache_clean() {
        let temp = TempDir::new().unwrap();
        let cache = Cache::new(temp.path());

        let task1 = task_id("app", "build");
        let task2 = task_id("lib", "build");

        cache
            .write(&task1, "hash1".to_string(), true, "cmd1".to_string())
            .unwrap();
        cache
            .write(&task2, "hash2".to_string(), true, "cmd2".to_string())
            .unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.entry_count, 2);

        let removed = cache.clean().unwrap();
        assert_eq!(removed, 2);

        let stats = cache.stats().unwrap();
        assert_eq!(stats.entry_count, 0);
    }

    #[test]
    fn test_cache_stats() {
        let temp = TempDir::new().unwrap();
        let mut cache = Cache::new(temp.path());

        let task = task_id("my-app", "build");
        let hash = "abc123".to_string();

        // Miss
        cache.check(&task, &hash);

        cache
            .write(&task, hash.clone(), true, "cargo build".to_string())
            .unwrap();

        // Hit
        cache.check(&task, &hash);

        let stats = cache.stats().unwrap();
        assert_eq!(stats.entry_count, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert!(stats.total_size > 0);
    }

    #[test]
    fn test_dependency_hashes_affect_input_hash() {
        let temp = TempDir::new().unwrap();
        let cache = Cache::new(temp.path());

        let hash1 = cache
            .compute_input_hash("cargo build", temp.path(), &[], &["dep_hash_1".to_string()])
            .unwrap();

        let hash2 = cache
            .compute_input_hash("cargo build", temp.path(), &[], &["dep_hash_2".to_string()])
            .unwrap();

        assert_ne!(hash1, hash2);
    }
}
