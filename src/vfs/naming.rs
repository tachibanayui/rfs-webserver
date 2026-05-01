use rand::RngExt;
use rand_xoshiro::Xoshiro256Plus;
use rand_xoshiro::rand_core::Rng;
use smallstr::SmallString;
use std::fmt::Write;
use std::marker::PhantomData;

/// Use for constructing strings in a hot loop. Cold string will still remains as std String.
pub type GenString = SmallString<[u8; 128]>;

use crate::dictionary::{Dictionary, IdFormat};

pub struct NameGenerator<'a, R = Xoshiro256Plus> {
    dictionary: &'a Dictionary,
    _rng: PhantomData<R>,
}

impl<'a, R> NameGenerator<'a, R>
where
    R: Rng,
{
    pub fn new(dictionary: &'a Dictionary) -> Self {
        Self {
            dictionary,
            _rng: PhantomData,
        }
    }

    pub fn directory_name(&self, rng: &mut R, depth: usize) -> &'a str {
        if depth == 0 {
            self.root_directory_name(rng)
        } else {
            self.nested_directory_name(rng)
        }
    }

    pub fn file_name(&self, rng: &mut R) -> GenString {
        let mut buf = GenString::with_capacity(128);
        let stem = pick_from(rng, self.dictionary.files.stems.iter());
        let extensions = self.dictionary.files.extensions.keys();
        let extension = pick_from(rng, extensions);
        let normalized_ext = extension.trim_start_matches('.');
        if normalized_ext.is_empty() {
            write!(&mut buf, "{stem}_").unwrap();
            self.generate_id(rng, &mut buf);
        } else {
            write!(&mut buf, "{stem}_").unwrap();
            self.generate_id(rng, &mut buf);
            write!(&mut buf, ".{normalized_ext}").unwrap();
        }

        buf
    }

    fn root_directory_name(&self, rng: &mut R) -> &'a str {
        let anchors_weight = self.dictionary.weights.anchors.unwrap_or(4);
        let common_weight = self.dictionary.weights.dirs_common.unwrap_or(1);
        let total = anchors_weight.saturating_add(common_weight).max(1);
        let choice = rng.random_range(0..total);

        if choice < anchors_weight {
            pick_from(rng, self.dictionary.anchors.roots.iter())
        } else {
            pick_from(rng, self.dictionary.dirs.common.iter())
        }
    }

    fn nested_directory_name(&self, rng: &mut R) -> &'a str {
        if self.dictionary.dirs.deep.is_empty() {
            return pick_from(rng, self.dictionary.dirs.common.iter());
        }

        let common_weight = self.dictionary.weights.dirs_common.unwrap_or(5);
        let deep_weight = self.dictionary.weights.dirs_deep.unwrap_or(2);
        let total = common_weight.saturating_add(deep_weight).max(1);
        let choice = rng.random_range(0..total);

        if choice < common_weight {
            pick_from(rng, self.dictionary.dirs.common.iter())
        } else {
            pick_from(rng, self.dictionary.dirs.deep.iter())
        }
    }

    fn generate_id(&self, rng: &mut R, buf: &mut GenString) {
        let format = pick_from(rng, self.dictionary.ids.formats.iter());
        match format {
            IdFormat::Uuid => uuid_like(rng, buf),
            IdFormat::Numeric => write!(buf, "{}", rng.random_range(10_000..=999_999)).unwrap(),
            IdFormat::Date => date_stamp(rng, buf),
            IdFormat::InvoiceCode => invoice_code(rng, buf),
        }
    }
}

fn pick_from<T, R: Rng>(rng: &mut R, mut values: impl ExactSizeIterator<Item = T>) -> T {
    let index = rng.random_range(0..values.len());
    values.nth(index).unwrap()
}

fn uuid_like<R: Rng>(rng: &mut R, buf: &mut GenString) {
    let mut bytes = [0u8; 16];
    rng.fill_bytes(&mut bytes);
    write!(
        buf,
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ).unwrap();
}

fn date_stamp<R: Rng>(rng: &mut R, buf: &mut GenString) {
    let year = rng.random_range(2024..=2026);
    let month = rng.random_range(1..=12);
    let day = rng.random_range(1..=28);
    write!(buf, "{year:04}-{month:02}-{day:02}").unwrap()
}

fn invoice_code<R: Rng>(rng: &mut R, buf: &mut GenString) {
    let year = rng.random_range(2024..=2026);
    let number = rng.random_range(100_000..=999_999);
    write!(buf, "INV-{year:04}-{number:06}").unwrap();
}
