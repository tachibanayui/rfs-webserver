use bytes::Bytes;
use futures::stream::{self, BoxStream};
use fxhash::{FxBuildHasher, FxHashMap, FxHashSet};
use quick_cache::sync::Cache;
use quick_cache::{OptionsBuilder, UnitWeighter};
use rand::RngExt;
use rand_xoshiro::Xoshiro256Plus;
use rand_xoshiro::rand_core::{Rng, SeedableRng};
use std::borrow::{Borrow, Cow};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::iter;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::fs::{self, File};
use tokio_util::io::ReaderStream;

use crate::cli::Config;
use crate::dictionary::SizeRange;
use crate::vfs::naming::NameGenerator;

#[derive(Debug, Clone)]
struct RealPathCache {
    data: FxHashMap<String, RealChildEntry>,
    ttl: Instant,
}

#[derive(Debug, Clone)]
struct FullPathCache {
    data: Option<Arc<DirectoryListing>>,
    ttl: Instant,
}

#[derive(Debug)]
struct FsCacheInner {
    allow_symlink: bool,
    config: Config,
    real_paths: Cache<PathBuf, Arc<RealPathCache>, UnitWeighter, FxBuildHasher>,
    full_paths: Cache<String, Arc<FullPathCache>, UnitWeighter, FxBuildHasher>,
}

#[derive(Debug, Clone)]
pub struct FsCache {
    inner: Arc<FsCacheInner>,
}

impl FsCache {
    async fn read_dir(&self, path: &Path) -> Arc<RealPathCache> {
        self.inner
            .real_paths
            .remove_if(path, |x| x.ttl > Instant::now());
        let item = self
            .inner
            .real_paths
            .get_or_insert_async(path, async {
                let items = real_children(path, self.inner.allow_symlink)
                    .await
                    .into_iter()
                    .map(|x| (x.name.clone(), x))
                    .collect();
                Ok::<_, Infallible>(
                    RealPathCache {
                        data: items,
                        ttl: Instant::now() + self.inner.config.fs_cache_ttl,
                    }
                    .into(),
                )
            })
            .await
            .unwrap();
        return item;
    }

    async fn read_vfs_dir<R>(&self, path: &str) -> Arc<FullPathCache>
    where
        R: Rng + SeedableRng + Send + 'static,
    {
        self.inner
            .full_paths
            .remove_if(path, |x| x.ttl > Instant::now());
        let item = self
            .inner
            .full_paths
            .get_or_insert_async(path, async {
                let listing = resolve_directory_path::<R>(&self, &self.inner.config, path).await;
                Ok::<_, Infallible>(Arc::new(FullPathCache {
                    data: listing.map(|x| x.into()),
                    ttl: Instant::now() + self.inner.config.fs_cache_ttl,
                }))
            })
            .await
            .unwrap();
        return item;
    }

    pub fn new(config: &Config) -> Self {
        let cache_opts = OptionsBuilder::new()
            .estimated_items_capacity(1000)
            .weight_capacity(1000)
            .build()
            .unwrap();
        let inner = FsCacheInner {
            allow_symlink: config.allow_symlink,
            config: config.clone(),
            real_paths: Cache::with_options(
                cache_opts.clone(),
                Default::default(),
                FxBuildHasher::new(),
                Default::default(),
            ),
            full_paths: Cache::with_options(
                cache_opts,
                Default::default(),
                FxBuildHasher::new(),
                Default::default(),
            ),
        };

        Self {
            inner: inner.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VirtualFilesystem<R = Xoshiro256Plus> {
    cache: FsCache,
    config: Config,
    _rng: PhantomData<R>,
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

pub struct FileEntry {
    pub stream: BoxStream<'static, Result<Bytes, std::io::Error>>,
    pub size_bytes: Option<u64>,
}

const GENERATED_CHUNK_SIZE: usize = 16 * 1024;

impl<R> VirtualFilesystem<R>
where
    R: Rng + SeedableRng + Send + 'static,
{
    pub async fn root_listing(&self) -> Arc<DirectoryListing> {
        self.directory_listing("/")
            .await
            .expect("root directory must always exist")
    }

    pub async fn directory_listing(&self, path: &str) -> Option<Arc<DirectoryListing>> {
        self.cache.read_vfs_dir::<R>(path).await.data.clone()
    }

    pub async fn file_entry(&self, path: &str) -> Option<FileEntry> {
        let trimmed = path.trim_end_matches('/');
        let Some(pos) = trimmed
            .as_bytes()
            .iter()
            .rev()
            .position(|x| *x == '/' as u8)
        else {
            return None;
        };
        let (parent, file) = trimmed.split_at(trimmed.len() - pos);
        let parent_listing = self.directory_listing(parent).await?;
        let child = parent_listing
            .children
            .iter()
            .find(|child| !child.is_directory && child.name == *file)?;

        match &child.source_path {
            Some(source_path) => {
                let file = File::open(&source_path).await.ok()?;
                let stream = Box::pin(ReaderStream::new(file));
                Some(FileEntry {
                    stream,
                    size_bytes: child.size_bytes,
                })
            }
            None => {
                let depth = parent.chars().filter(|x| *x == '/').count();
                let (stream, size_bytes) =
                    generated_file_stream::<R>(&self.config, &parent, file, depth);
                Some(FileEntry {
                    stream,
                    size_bytes: Some(size_bytes),
                })
            }
        }
    }
}

impl VirtualFilesystem<Xoshiro256Plus> {
    pub fn new(config: Config, cache: FsCache) -> Self {
        Self {
            config,
            cache,
            _rng: PhantomData,
        }
    }
}

struct SyntheticChildEntry<'a> {
    name: NodeStr<'a>,
    is_directory: bool,
}

impl<'a> SyntheticChildEntry<'a> {
    pub fn size_bytes(&self, config: &Config, path: &str, depth: usize) -> u64 {
        generated_file_size(config, path, depth, &self.name)
    }

    pub fn modified_unix_seconds(&self, config: &Config, path: &str, depth: usize) -> i64 {
        deterministic_modified_seconds(config.seed, path, depth, &self.name)
    }
}

type NodeStr<'a> = Cow<'a, str>;
type UniqueNameCache<'a> = FxHashSet<NodeStr<'a>>;

fn gen_synthetic_dir<'a, R>(
    config: &'a Config,
    path: &str,
    depth: usize,
    used_names: &mut UniqueNameCache<'a>,
) -> impl Iterator<Item = SyntheticChildEntry<'a>>
where
    R: Rng + SeedableRng + Send,
{
    let mut rng = directory_rng::<R>(config.seed, path, depth);
    let name_gen = NameGenerator::<R>::new(&config.dictionary);
    let file_count = rng.random_range(config.min_files..=config.max_files);
    let dir_count = if depth < config.depth {
        rng.random_range(config.min_dirs..=config.max_dirs)
    } else {
        0
    };

    iter::repeat(false)
        .take(file_count)
        .chain(iter::repeat(true).take(dir_count))
        .map(move |x| {
            let name = unique_name(&mut rng, used_names, |rng| {
                if x {
                    name_gen.directory_name(rng, depth).into()
                } else {
                    name_gen.file_name(rng).into()
                }
            });
            SyntheticChildEntry {
                name: name,
                is_directory: x,
            }
        })
}

fn get_selected_cand<R, T>(
    real_path_chance: f64,
    rng: &mut R,
    iter: impl Iterator<Item = T>,
) -> impl Iterator<Item = T>
where
    R: Rng + SeedableRng + Send,
    T: Borrow<RealChildEntry>,
{
    iter.filter(move |_| rng.random_bool(real_path_chance))
}

async fn resolve_directory_path<R>(
    fsc: &FsCache,
    config: &Config,
    path: &str,
) -> Option<DirectoryListing>
where
    R: Rng + SeedableRng + Send,
{
    let root_dir = match config.real_path.as_ref() {
        Some(x) => Some(fsc.read_dir(x).await),
        None => None,
    };

    let or = Default::default();
    let root_real_path = root_dir.as_ref().map(|x| &x.data).unwrap_or(&or);

    let mut iter = path.trim_matches('/').split('/').peekable();
    let mut current_path = String::from("");
    let mut depth = 0;
    let mut is_real_path = None;

    let mut used_names = FxHashSet::default();
    for seg in iter.by_ref() {
        if seg.is_empty() {
            continue;
        }

        if depth == config.depth {
            return None;
        }
        // User query a path have the same name with real path candidate
        // but we still need to check if it actually inside real path or the random generator generate a name
        // similar to real path
        if root_real_path.contains_key(seg) {
            let mut rng = directory_rng::<R>(config.seed, &current_path, depth);
            let is_real =
                get_selected_cand(config.real_path_chance, &mut rng, root_real_path.values())
                    .any(|x| x.name == seg && x.is_directory);
            if is_real {
                is_real_path.replace(seg);
                break;
            }
        }

        used_names.clear();
        let is_child = gen_synthetic_dir::<R>(config, &current_path, depth, &mut used_names)
            .any(|x| &*x.name == seg && x.is_directory);

        current_path.push('/');
        current_path.push_str(seg);
        if !is_child {
            return None;
        }

        depth += 1;
    }

    let Some(rps) = is_real_path else {
        used_names.clear();

        let mut children: Vec<_> =
            gen_synthetic_dir::<R>(config, &current_path, depth, &mut used_names)
                .map(|x| ChildEntry {
                    path: join_path(&current_path, &x.name),
                    size_bytes: (!x.is_directory)
                        .then(|| x.size_bytes(config, &current_path, depth)),
                    is_directory: x.is_directory,
                    source_path: None,
                    modified_unix_seconds: Some(x.modified_unix_seconds(
                        config,
                        &current_path,
                        depth,
                    )),
                    name: x.name.to_string(),
                })
                .collect();

        let mut rng = directory_rng::<R>(config.seed, &current_path, depth);
        children.extend(
            get_selected_cand(config.real_path_chance, &mut rng, root_real_path.values()).map(
                |x| ChildEntry {
                    path: join_path(&current_path, &x.name),
                    is_directory: x.is_directory,
                    source_path: Some(x.path.clone()),
                    size_bytes: x.size_bytes,
                    modified_unix_seconds: x.modified_unix_seconds,
                    name: x.name.clone(),
                },
            ),
        );

        children.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        return Some(DirectoryListing {
            path: current_path.to_string(),
            children,
        });
    };

    let mut pb = PathBuf::from(".");
    current_path.push('/');
    current_path.push_str(rps);
    pb.push(rps);
    for seg in iter.by_ref() {
        current_path.push('/');
        current_path.push_str(seg);
        pb.push(seg);
    }

    let mut children: Vec<_> = fsc
        .read_dir(&config.real_path.as_ref().unwrap().join(&pb))
        .await
        .data
        .values()
        .into_iter()
        .map(|x| ChildEntry {
            path: join_path(&current_path, &x.name),
            name: x.name.clone(),
            is_directory: x.is_directory,
            source_path: Some(x.path.clone()),
            size_bytes: x.size_bytes,
            modified_unix_seconds: x.modified_unix_seconds,
        })
        .collect();

    children.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    return Some(DirectoryListing {
        path: current_path.to_string(),
        children,
    });
}

fn generated_file_stream<R>(
    config: &Config,
    parent_path: &str,
    file_name: &str,
    depth: usize,
) -> (BoxStream<'static, Result<Bytes, std::io::Error>>, u64)
where
    R: Rng + SeedableRng + Send + 'static,
{
    let mut rng: R = file_rng::<R>(config.seed, parent_path, depth, file_name);
    let size_bytes = file_content_size(config, &mut rng, file_name);
    let stream = stream::unfold((rng, size_bytes), |(mut rng, remaining)| async move {
        if remaining == 0 {
            return None;
        }

        let chunk_len = if remaining < GENERATED_CHUNK_SIZE as u64 {
            remaining as usize
        } else {
            GENERATED_CHUNK_SIZE
        };
        let mut bytes = vec![0u8; chunk_len];
        for byte in bytes.iter_mut() {
            *byte = rng.random_range(32u8..=126u8);
        }
        Some((Ok(Bytes::from(bytes)), (rng, remaining - chunk_len as u64)))
    });

    (Box::pin(stream), size_bytes)
}

fn join_path(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{}", child)
    } else {
        format!("{}/{}", parent, child)
    }
}

fn directory_rng<R>(seed: u64, path: &str, depth: usize) -> R
where
    R: SeedableRng,
{
    R::seed_from_u64(stable_hash(seed, path, depth as u64))
}

fn file_rng<R>(seed: u64, path: &str, depth: usize, file_name: &str) -> R
where
    R: SeedableRng,
{
    let mut hash = stable_hash(seed, path, depth as u64);
    for byte in file_name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    R::seed_from_u64(hash)
}

fn file_content_size<R>(config: &Config, rng: &mut R, file_name: &str) -> u64
where
    R: Rng + SeedableRng,
{
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
        rng.random_range(min_size..=max_size)
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
    let mut rng: Xoshiro256Plus =
        file_rng::<Xoshiro256Plus>(config.seed, parent_path, depth, file_name);
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

fn random_suffix<R>(rng: &mut R) -> String
where
    R: Rng,
{
    format!("{:08x}", rng.next_u32())
}

fn unique_name<'a, R, F>(rng: &mut R, used: &mut UniqueNameCache<'a>, mut create: F) -> NodeStr<'a>
where
    R: Rng + SeedableRng,
    F: FnMut(&mut R) -> NodeStr<'a>,
{
    for _ in 0..10 {
        let candidate = create(rng);
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }

    let fallback: Cow<'_, _> = format!("{}-{}", &*create(rng), random_suffix(rng)).into();
    used.insert(fallback.clone());
    fallback
}

#[derive(Debug, Clone)]
pub struct RealChildEntry {
    name: String,
    path: PathBuf,
    is_directory: bool,
    size_bytes: Option<u64>,
    modified_unix_seconds: Option<i64>,
}

pub async fn real_children(source_path: &Path, allow_symlink: bool) -> Vec<RealChildEntry> {
    let mut children = Vec::new();

    let Ok(mut entries) = fs::read_dir(source_path).await else {
        return children;
    };

    loop {
        let item = entries.next_entry().await;
        let Ok(item) = item else {
            continue;
        };

        let Some(entry) = item else {
            break;
        };

        let Ok(file_type) = entry.file_type().await else {
            continue;
        };

        if !allow_symlink && file_type.is_symlink() {
            continue;
        }

        let Ok(metadata) = entry.metadata().await else {
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

    children.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::default_dictionary;
    use futures::StreamExt;

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
            allow_symlink: false,
            fs_cache_ttl: std::time::Duration::from_millis(3000),
            dictionary: default_dictionary(),
            footer_signature: "rfs-webserver/test".to_string(),
            delay: None,
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

    #[tokio::test]
    async fn directory_listings_are_deterministic_for_same_seed() {
        let config = config();
        let filesystem = VirtualFilesystem::new(config.clone(), FsCache::new(&config));
        let first = filesystem.directory_listing("/").await.unwrap();
        let second = filesystem.directory_listing("/").await.unwrap();

        assert_eq!(first.children.len(), second.children.len());
        assert_eq!(first.children[0].path, second.children[0].path);
    }

    #[tokio::test]
    async fn directory_depth_is_capped() {
        let config = config();
        let filesystem = VirtualFilesystem::new(config.clone(), FsCache::new(&config));
        let root = filesystem.root_listing().await;
        let first_directory = root
            .children
            .iter()
            .find(|child| child.is_directory)
            .expect("expected at least one directory");

        let child_listing = filesystem
            .directory_listing(&first_directory.path)
            .await
            .expect("child directory should exist");

        let grandchild_directory = child_listing
            .children
            .iter()
            .filter(|x| x.is_directory)
            .next()
            .expect("expected a nested directory at depth 1");

        let grandchild_listing = filesystem
            .directory_listing(&grandchild_directory.path)
            .await
            .expect("grandchild directory should exist");

        assert!(
            grandchild_listing
                .children
                .iter()
                .all(|child| !child.is_directory)
        );
    }

    async fn read_stream_to_string(mut stream: FileEntry) -> String {
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.stream.next().await {
            let chunk = chunk.expect("stream chunk should be readable");
            bytes.extend_from_slice(&chunk);
        }
        String::from_utf8(bytes).unwrap_or_default()
    }

    #[tokio::test]
    async fn real_entries_are_included_and_real_files_return_real_content() {
        let source = temp_dir("real-entries");
        write_file(&source.join("alpha.txt"), "alpha contents");
        write_file(&source.join("nested").join("child.txt"), "nested contents");

        let config = real_config(source.clone(), 1.0);
        let filesystem = VirtualFilesystem::new(config.clone(), FsCache::new(&config));
        let root = filesystem.root_listing().await;

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
            .await
            .expect("real file should resolve");

        assert_eq!(read_stream_to_string(file).await, "alpha contents");

        let nested_listing = filesystem
            .directory_listing(&nested.path)
            .await
            .expect("real directory should resolve");

        let child = nested_listing
            .children
            .iter()
            .find(|entry| entry.name == "child.txt")
            .expect("expected nested real file");

        let nested_file = filesystem
            .file_entry(dbg!(&child.path))
            .await
            .expect("nested real file should resolve");

        assert_eq!(read_stream_to_string(nested_file).await, "nested contents");
    }

    #[tokio::test]
    async fn real_mount_shows_only_real_contents_not_generated() {
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

        let filesystem = VirtualFilesystem::new(config.clone(), FsCache::new(&config));
        let root = filesystem.root_listing().await;

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
                .await
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
