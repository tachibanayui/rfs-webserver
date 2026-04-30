use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;

use crate::dictionary::{default_dictionary, Dictionary};

#[derive(Debug, Clone, Parser)]
#[command(author, version, about = "Random virtual filesystem webserver")]
pub struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: Ipv4Addr,

    #[arg(short, long, default_value_t = 3000)]
    pub port: u16,

    #[arg(long)]
    pub seed: Option<u64>,

    #[arg(long, default_value_t = 10)]
    pub depth: usize,

    #[arg(long, default_value_t = 10)]
    pub min_files: usize,

    #[arg(long, default_value_t = 100)]
    pub max_files: usize,

    #[arg(long, default_value_t = 0)]
    pub min_dirs: usize,

    #[arg(long, default_value_t = 100)]
    pub max_dirs: usize,

    #[arg(long, default_value = "./real-path")]
    pub real_path: Option<PathBuf>,

    #[arg(long, default_value_t = 1.0)]
    pub real_path_chance: f64,

    #[arg(long)]
    pub dictionary: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub seed: u64,
    pub depth: usize,
    pub min_files: usize,
    pub max_files: usize,
    pub min_dirs: usize,
    pub max_dirs: usize,
    pub real_path: Option<PathBuf>,
    pub real_path_chance: f64,
    pub dictionary: Dictionary,
}

impl Args {
    pub fn into_config(self) -> Result<Config, String> {
        if self.min_files > self.max_files {
            return Err("min-files cannot be greater than max-files".to_string());
        }

        if self.min_dirs > self.max_dirs {
            return Err("min-dirs cannot be greater than max-dirs".to_string());
        }

        if !(0.0..=1.0).contains(&self.real_path_chance) {
            return Err("real-path-chance must be between 0 and 1".to_string());
        }

        let seed = self.seed.unwrap_or_else(current_seed);
        let real_path = match self.real_path {
            Some(path) => Some(validate_real_path(path)?),
            None => None,
        };
        let dictionary = match self.dictionary {
            Some(path) => Dictionary::from_path(&path)?,
            None => default_dictionary(),
        };

        Ok(Config {
            host: self.host,
            port: self.port,
            seed,
            depth: self.depth,
            min_files: self.min_files,
            max_files: self.max_files,
            min_dirs: self.min_dirs,
            max_dirs: self.max_dirs,
            real_path,
            real_path_chance: self.real_path_chance,
            dictionary,
        })
    }
}

fn current_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default()
}

fn validate_real_path(path: PathBuf) -> Result<PathBuf, String> {
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("real-path does not exist or cannot be read: {error}"))?;

    if !metadata.is_dir() {
        return Err("real-path must point to a directory".to_string());
    }

    std::fs::canonicalize(&path)
        .map_err(|error| format!("real-path could not be resolved: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = format!(
            "rfs-webserver-{}-{}-{}",
            name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("temp dir should be creatable");
        path
    }

    #[test]
    fn into_config_rejects_real_path_chance_outside_range() {
        let args = Args {
            host: Ipv4Addr::LOCALHOST,
            port: 3000,
            seed: Some(1),
            depth: 2,
            min_files: 1,
            max_files: 2,
            min_dirs: 0,
            max_dirs: 1,
            real_path: None,
            real_path_chance: 1.5,
            dictionary: None,
        };

        assert!(args.into_config().is_err());
    }

    #[test]
    fn into_config_canonicalizes_real_path() {
        let dir = temp_dir("canonicalize");
        let args = Args {
            host: Ipv4Addr::LOCALHOST,
            port: 3000,
            seed: Some(1),
            depth: 2,
            min_files: 1,
            max_files: 2,
            min_dirs: 0,
            max_dirs: 1,
            real_path: Some(dir.clone()),
            real_path_chance: 0.5,
            dictionary: None,
        };

        let config = args.into_config().expect("config should validate");

        assert_eq!(config.real_path, Some(std::fs::canonicalize(dir).unwrap()));
        assert_eq!(config.real_path_chance, 0.5);
    }
}
