use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::Config;
use crate::dictionary::SizeRange;
use crate::vfs::naming::NameGenerator;

#[derive(Debug, Clone)]
pub struct VirtualFilesystem {
    config: Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub source_path: Option<PathBuf>,
    pub size_bytes: Option<u64>,
    pub modified_unix_seconds: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct DirectoryListing {
    pub path: String,
    pub children: Vec<ChildEntry>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub content: String,
}

impl VirtualFilesystem {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn root_listing(&self) -> DirectoryListing {
        self.directory_listing("/")
            .expect("root directory must always exist")
    }

    pub fn directory_listing(&self, path: &str) -> Option<DirectoryListing> {
        let normalized = normalize_directory_path(path)?;
        let segments = path_segments(&normalized);
        resolve_directory_path(&self.config, &segments)
    }

    pub fn file_entry(&self, path: &str) -> Option<FileEntry> {
        let normalized = normalize_file_path(path)?;
        let segments = path_segments(&normalized);
        let (file_name, parent_segments) = segments.split_last()?;
        let parent_path = segments_to_path(parent_segments);
        let parent_listing = self.directory_listing(&parent_path)?;

        let child = parent_listing
            .children
            .into_iter()
            .find(|child| !child.is_directory && child.name == *file_name)?;

        Some(FileEntry {
            content: match child.source_path {
                Some(source_path) => fs::read_to_string(source_path).ok()?,
                None => render_file_content(
                    &self.config,
                    &parent_path,
                    file_name,
                    parent_segments.len(),
                ),
            },
        })
    }
}

fn build_listing(
    config: &Config,
    path: &str,
    depth: usize,
    source_path: Option<PathBuf>,
) -> DirectoryListing {
    let mut children = Vec::new();

    // If we're inside a mounted real directory, list its actual contents
    if let Some(source_path) = source_path.as_ref() {
        for real_child in real_children(source_path) {
            let child_path = join_path(path, &real_child.name);
            children.push(ChildEntry {
                name: real_child.name,
                path: child_path,
                is_directory: real_child.is_directory,
                source_path: Some(real_child.path),
                size_bytes: real_child.size_bytes,
                modified_unix_seconds: real_child.modified_unix_seconds,
            });
        }
    } else {
        // Otherwise, generate synthetic VFS entries
        let mut rng = directory_rng(config.seed, path, depth);

        let name_generator = NameGenerator::new(&config.dictionary);
        let mut used_names = HashSet::new();

        let file_count = rng.gen_range(config.min_files..=config.max_files);
        for _ in 0..file_count {
            let name = unique_name(&mut rng, &mut used_names, |rng| {
                name_generator.file_name(rng)
            });
            children.push(ChildEntry {
                name: name.clone(),
                path: join_path(path, &name),
                is_directory: false,
                source_path: None,
                size_bytes: Some(generated_file_size(config, path, depth, &name)),
                modified_unix_seconds: Some(deterministic_modified_seconds(
                    config.seed,
                    path,
                    depth,
                    &name,
                )),
            });
        }

        if depth < config.depth {
            let directory_count = rng.gen_range(config.min_dirs..=config.max_dirs);
            for _ in 0..directory_count {
                let name = unique_name(&mut rng, &mut used_names, |rng| {
                    name_generator.directory_name(rng, depth)
                });
                children.push(ChildEntry {
                    name: name.clone(),
                    path: join_path(path, &name),
                    is_directory: true,
                    source_path: None,
                    size_bytes: None,
                    modified_unix_seconds: Some(deterministic_modified_seconds(
                        config.seed,
                        path,
                        depth,
                        &name,
                    )),
                });
            }
        }

        // At root level, optionally include real entries as mounts
        if path == "/" && config.real_path.is_some() {
            let mut rng = directory_rng(config.seed, path, depth);
            if let Some(real_root) = config.real_path.as_ref() {
                for real_child in real_children(real_root) {
                    if !rng.gen_bool(config.real_path_chance) {
                        continue;
                    }

                    let child_path = join_path(path, &real_child.name);
                    children.push(ChildEntry {
                        name: real_child.name,
                        path: child_path,
                        is_directory: real_child.is_directory,
                        source_path: Some(real_child.path),
                        size_bytes: real_child.size_bytes,
                        modified_unix_seconds: real_child.modified_unix_seconds,
                    });
                }
            }
        }
    }

    children.sort_by(|left, right| left.path.cmp(&right.path));

    DirectoryListing {
        path: path.to_string(),
        children,
    }
}

fn resolve_directory_path(config: &Config, segments: &[&str]) -> Option<DirectoryListing> {
    let mut current_path = String::from("/");
    let mut source_path: Option<PathBuf> = None;
    let mut depth = 0;

    for segment in segments {
        let listing = build_listing(config, &current_path, depth, source_path.clone());
        let child = listing
            .children
            .iter()
            .find(|candidate| candidate.is_directory && candidate.name == *segment)?;
        current_path = child.path.clone();
        source_path = child.source_path.clone();
        depth += 1;
    }

    Some(build_listing(config, &current_path, depth, source_path))
}

fn render_file_content(
    config: &Config,
    parent_path: &str,
    file_name: &str,
    depth: usize,
) -> String {
    let mut rng = file_rng(config.seed, parent_path, depth, file_name);
    let size = file_content_size(config, &mut rng, file_name);
    let size = usize::try_from(size).unwrap_or(usize::MAX);

    if size == 0 {
        return String::new();
    }

    let mut bytes = vec![0u8; size];
    for byte in bytes.iter_mut() {
        *byte = rng.gen_range(32u8..=126u8);
    }

    String::from_utf8(bytes).unwrap_or_default()
}

fn normalize_directory_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        Some("/".to_string())
    } else {
        Some(format!("/{}", trimmed))
    }
}

fn normalize_file_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(format!("/{}", trimmed))
    }
}

fn path_segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn segments_to_path(segments: &[&str]) -> String {
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn join_path(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{}", child)
    } else {
        format!("{}/{}", parent, child)
    }
}

fn directory_rng(seed: u64, path: &str, depth: usize) -> StdRng {
    StdRng::seed_from_u64(stable_hash(seed, path, depth as u64))
}

fn file_rng(seed: u64, path: &str, depth: usize, file_name: &str) -> StdRng {
    let mut hash = stable_hash(seed, path, depth as u64);
    for byte in file_name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    StdRng::seed_from_u64(hash)
}

fn file_content_size(config: &Config, rng: &mut StdRng, file_name: &str) -> u64 {
    let extension = file_extension(file_name);
    let range = extension
        .and_then(|ext| lookup_extension_range(&config.dictionary.files.extensions, ext))
        .or_else(|| config.dictionary.files.extensions.values().next())
        .expect("dictionary requires at least one extension");

    let min_size = range.min_size.value();
    let max_size = range.max_size.value();
    if min_size >= max_size {
        min_size
    } else {
        rng.gen_range(min_size..=max_size)
    }
}

fn file_extension(file_name: &str) -> Option<&str> {
    file_name.rsplit_once('.').map(|(_, ext)| ext.trim())
}

fn lookup_extension_range<'a>(
    extensions: &'a BTreeMap<String, SizeRange>,
    extension: &str,
) -> Option<&'a SizeRange> {
    if let Some(range) = extensions.get(extension) {
        return Some(range);
    }

    let trimmed = extension.trim_start_matches('.');
    if let Some(range) = extensions.get(trimmed) {
        return Some(range);
    }

    let lower = trimmed.to_ascii_lowercase();
    if let Some(range) = extensions.get(&lower) {
        return Some(range);
    }

    let dotted = format!(".{trimmed}");
    if let Some(range) = extensions.get(&dotted) {
        return Some(range);
    }

    let dotted_lower = format!(".{lower}");
    extensions.get(&dotted_lower)
}

fn generated_file_size(config: &Config, parent_path: &str, depth: usize, file_name: &str) -> u64 {
    let mut rng = file_rng(config.seed, parent_path, depth, file_name);
    file_content_size(config, &mut rng, file_name)
}

fn deterministic_modified_seconds(seed: u64, path: &str, depth: usize, name: &str) -> i64 {
    let mut hash = stable_hash(seed, path, depth as u64);
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }

    let base_timestamp = 1_640_995_200u64; // 2022-01-01 00:00:00 UTC
    let span_seconds = 3 * 365 * 24 * 60 * 60u64;
    let offset = hash % span_seconds;
    (base_timestamp + offset) as i64
}

fn system_time_to_unix_seconds(value: SystemTime) -> Option<i64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

fn stable_hash(seed: u64, path: &str, depth: u64) -> u64 {
    let mut hash = seed ^ 0x9e37_79b9_7f4a_7c15;

    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }

    hash ^= depth.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash
}

fn random_suffix(rng: &mut StdRng) -> String {
    format!("{:08x}", rng.next_u32())
}

fn unique_name<F>(rng: &mut StdRng, used: &mut HashSet<String>, mut create: F) -> String
where
    F: FnMut(&mut StdRng) -> String,
{
    for _ in 0..10 {
        let candidate = create(rng);
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }

    let fallback = format!("{}-{}", create(rng), random_suffix(rng));
    used.insert(fallback.clone());
    fallback
}

#[derive(Debug, Clone)]
struct RealChildEntry {
    name: String,
    path: PathBuf,
    is_directory: bool,
    size_bytes: Option<u64>,
    modified_unix_seconds: Option<i64>,
}

fn real_children(source_path: &Path) -> Vec<RealChildEntry> {
    let mut children = Vec::new();

    let Ok(entries) = fs::read_dir(source_path) else {
        return children;
    };

    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        let path = entry.path();
        let Some(name) = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
        else {
            continue;
        };

        let is_directory = metadata.is_dir();
        let size_bytes = if is_directory {
            None
        } else {
            Some(metadata.len())
        };
        let modified_unix_seconds = metadata
            .modified()
            .ok()
            .and_then(system_time_to_unix_seconds);

        children.push(RealChildEntry {
            name,
            path,
            is_directory,
            size_bytes,
            modified_unix_seconds,
        });
    }

    children.sort_by(|left, right| left.name.cmp(&right.name));
    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::default_dictionary;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = format!(
            "rfs-webserver-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("temp dir should be creatable");
        path
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir should be creatable");
        }

        std::fs::write(path, contents).expect("file should be writable");
    }

    fn config() -> Config {
        Config {
            host: std::net::Ipv4Addr::LOCALHOST,
            port: 3000,
            seed: 1234,
            depth: 2,
            min_files: 1,
            max_files: 2,
            min_dirs: 1,
            max_dirs: 2,
            real_path: None,
            real_path_chance: 0.0,
            dictionary: default_dictionary(),
            footer_signature: "rfs-webserver/test".to_string(),
        }
    }

    fn real_config(real_path: PathBuf, chance: f64) -> Config {
        let mut config = config();
        config.real_path = Some(real_path);
        config.real_path_chance = chance;
        config.min_files = 0;
        config.max_files = 0;
        config.min_dirs = 0;
        config.max_dirs = 0;
        config.depth = 4;
        config
    }

    #[test]
    fn directory_listings_are_deterministic_for_same_seed() {
        let filesystem = VirtualFilesystem::new(config());
        let first = filesystem.directory_listing("/").unwrap();
        let second = filesystem.directory_listing("/").unwrap();

        assert_eq!(first.children.len(), second.children.len());
        assert_eq!(first.children[0].path, second.children[0].path);
    }

    #[test]
    fn directory_depth_is_capped() {
        let filesystem = VirtualFilesystem::new(config());
        let root = filesystem.root_listing();
        let first_directory = root
            .children
            .iter()
            .find(|child| child.is_directory)
            .expect("expected at least one directory");

        let child_listing = filesystem
            .directory_listing(&first_directory.path)
            .expect("child directory should exist");

        let grandchild_directory = child_listing
            .children
            .iter()
            .find(|child| child.is_directory)
            .expect("expected a nested directory at depth 1");

        let grandchild_listing = filesystem
            .directory_listing(&grandchild_directory.path)
            .expect("grandchild directory should exist");

        assert!(
            grandchild_listing
                .children
                .iter()
                .all(|child| !child.is_directory)
        );
    }

    #[test]
    fn generated_names_are_not_template_like() {
        let filesystem = VirtualFilesystem::new(config());
        let root = filesystem.root_listing();

        assert!(
            !root
                .children
                .iter()
                .any(|child| child.name.starts_with("dir-") || child.name.starts_with("file-"))
        );
    }

    #[test]
    fn real_entries_are_included_and_real_files_return_real_content() {
        let source = temp_dir("real-entries");
        write_file(&source.join("alpha.txt"), "alpha contents");
        write_file(&source.join("nested").join("child.txt"), "nested contents");

        let filesystem = VirtualFilesystem::new(real_config(source.clone(), 1.0));
        let root = filesystem.root_listing();

        let alpha = root
            .children
            .iter()
            .find(|child| child.name == "alpha.txt")
            .expect("expected real file in root listing");

        assert!(!alpha.is_directory);
        assert!(alpha.source_path.is_some());

        let nested = root
            .children
            .iter()
            .find(|child| child.name == "nested")
            .expect("expected real directory in root listing");

        assert!(nested.is_directory);
        assert!(nested.source_path.is_some());

        let file = filesystem
            .file_entry(&alpha.path)
            .expect("real file should resolve");

        assert_eq!(file.content, "alpha contents");

        let nested_listing = filesystem
            .directory_listing(&nested.path)
            .expect("real directory should resolve");

        let child = nested_listing
            .children
            .iter()
            .find(|entry| entry.name == "child.txt")
            .expect("expected nested real file");

        let nested_file = filesystem
            .file_entry(&child.path)
            .expect("nested real file should resolve");

        assert_eq!(nested_file.content, "nested contents");
    }

    #[test]
    fn real_mount_shows_only_real_contents_not_generated() {
        let source = temp_dir("mount-test");
        write_file(&source.join("real-file.txt"), "real content");
        write_file(&source.join("real-subdir").join("nested.txt"), "nested");

        let mut config = config();
        config.real_path = Some(source.clone());
        config.real_path_chance = 1.0;
        config.min_files = 5;
        config.max_files = 10;
        config.min_dirs = 2;
        config.max_dirs = 5;

        let filesystem = VirtualFilesystem::new(config);
        let root = filesystem.root_listing();

        // Real path contents should appear at root
        let real_file = root
            .children
            .iter()
            .find(|child| child.name == "real-file.txt");
        let real_subdir = root
            .children
            .iter()
            .find(|child| child.name == "real-subdir");

        assert!(real_file.is_some(), "real file should appear at root");
        assert!(real_subdir.is_some(), "real subdir should appear at root");

        // Entering the mounted real subdir should show ONLY its contents
        if let Some(subdir) = real_subdir.filter(|e| e.is_directory) {
            let subdir_listing = filesystem
                .directory_listing(&subdir.path)
                .expect("mounted real dir should resolve");

            let nested = subdir_listing
                .children
                .iter()
                .find(|child| child.name == "nested.txt");

            assert!(nested.is_some(), "nested file should appear in mounted dir");

            // Should not have generated dir names (like "dir-...")
            assert!(
                !subdir_listing
                    .children
                    .iter()
                    .any(|child| child.name.starts_with("dir-")),
                "mounted dir should not contain generated entries"
            );
        } else {
            panic!("real-subdir should be a directory");
        }
    }
}
