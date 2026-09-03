//! The robustness gate: every parser that takes bytes from a wire, a store
//! or a bus returns an error on bad input; it never panics. Random inputs
//! from an LCG (the same corpus on every machine) and mutations of a valid
//! encoding (bit flips, truncation, extension), each run through the parser
//! under `catch_unwind` so a failure names the parser and prints the input.

use std::panic::{AssertUnwindSafe, catch_unwind};

use rusty_esp_core::capability::{Capability, Chip, Manifest, ParsedManifest, Status};
use rusty_esp_core::time::{Micros, WallOffset};

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn bytes(&mut self, max_len: usize) -> Vec<u8> {
        let n = self.below(max_len + 1);
        (0..n).map(|_| (self.next() >> 56) as u8).collect()
    }

    /// One of: a bit flip, a byte overwrite, a truncation, an extension, a
    /// byte insertion, a byte removal.
    fn mutate(&mut self, base: &[u8]) -> Vec<u8> {
        let mut v = base.to_vec();
        match self.below(6) {
            0 if !v.is_empty() => {
                let i = self.below(v.len());
                v[i] ^= 1 << self.below(8);
            }
            1 if !v.is_empty() => {
                let i = self.below(v.len());
                v[i] = (self.next() >> 56) as u8;
            }
            2 => v.truncate(self.below(v.len() + 1)),
            3 => {
                let extra = self.bytes(16);
                v.extend_from_slice(&extra);
            }
            4 => {
                let i = self.below(v.len() + 1);
                v.insert(i, (self.next() >> 56) as u8);
            }
            _ if !v.is_empty() => {
                let i = self.below(v.len());
                v.remove(i);
            }
            _ => {}
        }
        v
    }
}

fn check<R>(name: &str, input: &[u8], f: impl FnOnce() -> R) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        let hex: String = input.iter().map(|b| format!("{b:02x}")).collect();
        panic!("{name} panicked on {} bytes: {hex}", input.len());
    }
}

#[test]
fn manifest_parsers_never_panic() {
    let mut rng = Lcg(0xC0DE_0001);
    let m = Manifest {
        model: "acme/doorbell-2",
        firmware: "1.4.0",
        chip: Chip::Esp32S3,
        declared: &[],
    };
    let mut buf = [0u8; 512];
    let n = m.encode(&mut buf).unwrap();
    let valid = buf[..n].to_vec();
    assert!(Manifest::parse(&valid).is_ok());
    for i in 0..30_000 {
        let input = if i % 3 == 0 {
            rng.bytes(600)
        } else {
            rng.mutate(&valid)
        };
        check("Manifest::parse", &input, || {
            Manifest::parse(&input).map(|_| ())
        });
        check("ParsedManifest::parse", &input, || {
            ParsedManifest::parse(&input).map(|p| {
                let mut out = [0u8; 1024];
                let _ = p.encode(&mut out);
            })
        });
    }
}

#[test]
fn tag_parsers_never_panic() {
    let mut rng = Lcg(0xC0DE_0002);
    for _ in 0..30_000 {
        let bytes = rng.bytes(24);
        let s = String::from_utf8_lossy(&bytes);
        check("Capability::parse", &bytes, || Capability::parse(&s));
        check("Status::parse", &bytes, || Status::parse(&s));
        check("Chip::parse", &bytes, || Chip::parse(&s));
    }
}

#[test]
fn time_arithmetic_never_panics() {
    let mut rng = Lcg(0xC0DE_0003);
    for _ in 0..100_000 {
        let (a, b, c) = (rng.next(), rng.next(), rng.next());
        // extremes as well as the random middle
        let pick = |x: u64, k: u64| match k % 4 {
            0 => 0,
            1 => u64::MAX,
            2 => x >> 32,
            _ => x,
        };
        let (send, device, recv) = (pick(a, b), pick(b, c), pick(c, a));
        check("WallOffset::from_exchange", &[], || {
            if let Ok(w) = WallOffset::from_exchange(send, Micros(device), recv) {
                let _ = w.to_wall(Micros(pick(a, c)));
                let _ = w.error_us();
            }
        });
    }
}
