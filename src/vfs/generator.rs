use crate::cli::Config;

use super::node::VirtualFilesystem;

pub fn generate(config: &Config) -> VirtualFilesystem {
    VirtualFilesystem::new(config.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_keeps_only_configuration_state() {
        let filesystem = generate(&Config {
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
        });

        assert!(filesystem.root_listing().children.len() >= 2);
    }

    #[test]
    fn generation_is_deterministic_for_same_seed() {
        let config = Config {
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
        };

        let first = generate(&config);
        let second = generate(&config);

        assert_eq!(
            first.root_listing().children,
            second.root_listing().children
        );
    }
}
