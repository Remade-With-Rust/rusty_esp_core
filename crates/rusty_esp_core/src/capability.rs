//! The capability manifest — what a device can do, stated honestly.
//!
//! This is the vendor contract of the Janus family. A device declares each
//! capability with a **status** and the **crate that backs it**, exactly the
//! way the MATA OEM sidecar's catalog does: an *available* capability must
//! name its backing crate, a *planned* one must not. `rusty_esp_mid` signs the
//! canonical encoding so the home computer's catalog can trust it; the
//! encoding is deterministic and needs neither `serde` nor `alloc`.
//!
//! Wire tags are stable forever. Add variants; never rename one.

use crate::error::{Error, Result};

/// A thing a Janus device can do. Tags are the wire form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Capability {
    /// Still images as JPEG.
    ImageJpeg,
    /// Still images as raw pixels (the manifest's `PixelFormat` list says which).
    ImageRaw,
    /// Motion JPEG stream.
    VideoMjpeg,
    /// H.264 stream.
    VideoH264,
    /// Microphone capture.
    AudioPcmIn,
    /// Speaker / line playback.
    AudioPcmOut,
    /// Opus-coded audio.
    AudioOpus,
    /// FLAC-coded audio.
    AudioFlac,
    /// Wi-Fi station.
    WifiStation,
    /// Wi-Fi soft access point (provisioning or standalone).
    WifiSoftAp,
    /// Wi-Fi channel state information export.
    WifiCsi,
    /// Presence / motion inferred from Wi-Fi CSI.
    RadarPresence,
    /// A UART mmWave radar module (LD2410 class).
    RadarMmWave,
    /// BLE GATT services.
    BleGatt,
    /// ESP-NOW frames.
    EspNow,
    /// LoRa point-to-point.
    LoraP2p,
    /// LoRaWAN end device.
    LoraWan,
    /// iroh endpoint reachable on the LAN.
    IrohLanDirect,
    /// iroh endpoint reachable through a relay (needs PSRAM).
    IrohRelay,
    /// The device holds its own `did:mata` key.
    MidDevice,
    /// The device has been adopted by an owner (a signed adoption is stored).
    MidAdopted,
    /// General-purpose digital I/O exposed as ops.
    Gpio,
    /// Periodic telemetry (uptime, RSSI, temperature).
    Telemetry,
    /// Over-the-air firmware update.
    Ota,
}

impl Capability {
    /// Every capability, in wire order.
    pub const ALL: &'static [Capability] = &[
        Capability::ImageJpeg,
        Capability::ImageRaw,
        Capability::VideoMjpeg,
        Capability::VideoH264,
        Capability::AudioPcmIn,
        Capability::AudioPcmOut,
        Capability::AudioOpus,
        Capability::AudioFlac,
        Capability::WifiStation,
        Capability::WifiSoftAp,
        Capability::WifiCsi,
        Capability::RadarPresence,
        Capability::RadarMmWave,
        Capability::BleGatt,
        Capability::EspNow,
        Capability::LoraP2p,
        Capability::LoraWan,
        Capability::IrohLanDirect,
        Capability::IrohRelay,
        Capability::MidDevice,
        Capability::MidAdopted,
        Capability::Gpio,
        Capability::Telemetry,
        Capability::Ota,
    ];

    /// The stable wire tag.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Capability::ImageJpeg => "image.jpeg",
            Capability::ImageRaw => "image.raw",
            Capability::VideoMjpeg => "video.mjpeg",
            Capability::VideoH264 => "video.h264",
            Capability::AudioPcmIn => "audio.pcm.in",
            Capability::AudioPcmOut => "audio.pcm.out",
            Capability::AudioOpus => "audio.opus",
            Capability::AudioFlac => "audio.flac",
            Capability::WifiStation => "wifi.sta",
            Capability::WifiSoftAp => "wifi.ap",
            Capability::WifiCsi => "wifi.csi",
            Capability::RadarPresence => "radar.presence",
            Capability::RadarMmWave => "radar.mmwave",
            Capability::BleGatt => "ble.gatt",
            Capability::EspNow => "espnow",
            Capability::LoraP2p => "lora.p2p",
            Capability::LoraWan => "lora.wan",
            Capability::IrohLanDirect => "iroh.lan",
            Capability::IrohRelay => "iroh.relay",
            Capability::MidDevice => "mid.device",
            Capability::MidAdopted => "mid.adopted",
            Capability::Gpio => "gpio",
            Capability::Telemetry => "telemetry",
            Capability::Ota => "ota",
        }
    }

    /// Parse a wire tag.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Capability> {
        Capability::ALL.iter().copied().find(|c| c.tag() == tag)
    }
}

/// How real a declared capability is. Copy the status; never upgrade it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// Shipped, tested against its kill test, backed by a named crate.
    Available,
    /// Wired and demonstrable, not yet through its kill test.
    Preview,
    /// Intended; no code claims it.
    Planned,
}

impl Status {
    /// The stable wire tag.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Status::Available => "available",
            Status::Preview => "preview",
            Status::Planned => "planned",
        }
    }

    /// Parse a wire tag.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Status> {
        match tag {
            "available" => Some(Status::Available),
            "preview" => Some(Status::Preview),
            "planned" => Some(Status::Planned),
            _ => None,
        }
    }
}

/// One line of a manifest: a capability, its status, and the crate behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Declared {
    /// What.
    pub capability: Capability,
    /// How real.
    pub status: Status,
    /// The crate that implements it (`""` when planned).
    pub backing: &'static str,
}

impl Declared {
    /// A shipped capability backed by `backing`.
    #[must_use]
    pub const fn available(capability: Capability, backing: &'static str) -> Self {
        Declared {
            capability,
            status: Status::Available,
            backing,
        }
    }

    /// A demonstrable capability backed by `backing`.
    #[must_use]
    pub const fn preview(capability: Capability, backing: &'static str) -> Self {
        Declared {
            capability,
            status: Status::Preview,
            backing,
        }
    }

    /// An intended capability with no code behind it.
    #[must_use]
    pub const fn planned(capability: Capability) -> Self {
        Declared {
            capability,
            status: Status::Planned,
            backing: "",
        }
    }

    /// The honesty rule: live claims name their code; planned claims name none.
    pub fn validate(&self) -> Result<()> {
        let ok = match self.status {
            Status::Available | Status::Preview => !self.backing.is_empty(),
            Status::Planned => self.backing.is_empty(),
        };
        let clean = self
            .backing
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if ok && clean {
            Ok(())
        } else {
            Err(Error::InvalidFormat)
        }
    }
}

/// The Espressif part the firmware runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Chip {
    /// ESP32 (Xtensa LX6, dual core).
    Esp32,
    /// ESP32-S2 (Xtensa LX7, single core).
    Esp32S2,
    /// ESP32-S3 (Xtensa LX7, dual core, PIE SIMD).
    Esp32S3,
    /// ESP32-C3 (RISC-V).
    Esp32C3,
    /// ESP32-C5 (RISC-V, dual-band Wi-Fi).
    Esp32C5,
    /// ESP32-C6 (RISC-V, Wi-Fi 6, 802.15.4).
    Esp32C6,
    /// ESP32-C61 (RISC-V).
    Esp32C61,
    /// ESP32-H2 (RISC-V, 802.15.4 only).
    Esp32H2,
    /// ESP32-P4 (RISC-V dual core, PIE SIMD, hardware JPEG/H.264, no radio).
    Esp32P4,
}

impl Chip {
    /// The stable wire tag (the esp-hal / espflash chip name).
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Chip::Esp32 => "esp32",
            Chip::Esp32S2 => "esp32s2",
            Chip::Esp32S3 => "esp32s3",
            Chip::Esp32C3 => "esp32c3",
            Chip::Esp32C5 => "esp32c5",
            Chip::Esp32C6 => "esp32c6",
            Chip::Esp32C61 => "esp32c61",
            Chip::Esp32H2 => "esp32h2",
            Chip::Esp32P4 => "esp32p4",
        }
    }

    /// True when the part has an on-die Wi-Fi radio.
    #[must_use]
    pub const fn has_wifi(self) -> bool {
        !matches!(self, Chip::Esp32H2 | Chip::Esp32P4)
    }

    /// True when the part carries the PIE SIMD extension.
    #[must_use]
    pub const fn has_pie(self) -> bool {
        matches!(self, Chip::Esp32S3 | Chip::Esp32P4)
    }

    /// Every part, in tag order.
    pub const ALL: &'static [Chip] = &[
        Chip::Esp32,
        Chip::Esp32S2,
        Chip::Esp32S3,
        Chip::Esp32C3,
        Chip::Esp32C5,
        Chip::Esp32C6,
        Chip::Esp32C61,
        Chip::Esp32H2,
        Chip::Esp32P4,
    ];

    /// Parse a wire tag (`esp32s3`).
    #[must_use]
    pub fn parse(tag: &str) -> Option<Chip> {
        Chip::ALL.iter().copied().find(|c| c.tag() == tag)
    }
}

/// Longest `model` or `firmware` field accepted, in bytes.
pub const MAX_FIELD_LEN: usize = 64;

/// A device's capability manifest, over caller-owned declarations.
#[derive(Debug, Clone, Copy)]
pub struct Manifest<'a> {
    /// The maker's model string, e.g. `acme/doorbell-2`.
    pub model: &'a str,
    /// Firmware version, e.g. `1.4.0`.
    pub firmware: &'a str,
    /// The part.
    pub chip: Chip,
    /// Everything the device declares.
    pub declared: &'a [Declared],
}

impl<'a> Manifest<'a> {
    /// True when the manifest declares `capability` as available.
    #[must_use]
    pub fn has(&self, capability: Capability) -> bool {
        self.declared
            .iter()
            .any(|d| d.capability == capability && d.status == Status::Available)
    }

    /// Validate every line and the field lengths. A duplicate capability is
    /// an error: one device, one truth per capability.
    pub fn validate(&self) -> Result<()> {
        check_field(self.model)?;
        check_field(self.firmware)?;
        for (i, d) in self.declared.iter().enumerate() {
            d.validate()?;
            if self.declared[..i]
                .iter()
                .any(|e| e.capability == d.capability)
            {
                return Err(Error::InvalidFormat);
            }
        }
        Ok(())
    }

    /// Canonical encoding, written into `out`; returns the byte count.
    ///
    /// The form is line-oriented text, one `key=value` per line, declarations
    /// in [`Capability::ALL`] order regardless of the order given, so the same
    /// facts always produce the same bytes — which is what makes the signature
    /// `rusty_esp_mid` puts over it meaningful:
    ///
    /// ```text
    /// janus/1
    /// model=acme/doorbell-2
    /// fw=1.4.0
    /// chip=esp32s3
    /// cap=image.jpeg:available:rusty_esp_image
    /// cap=iroh.relay:planned:
    /// ```
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        self.validate()?;
        encode_lines(out, self.model, self.firmware, self.chip, |cap| {
            self.declared
                .iter()
                .find(|d| d.capability == cap)
                .map(|d| (d.status, d.backing))
        })
    }

    /// Read a canonical encoding back into an owned [`ParsedManifest`]
    /// (`alloc`). Strict: the lines must be in canonical order, every tag
    /// known, no duplicates, so `parse` then `encode` reproduces the bytes.
    #[cfg(feature = "alloc")]
    pub fn parse(bytes: &[u8]) -> Result<ParsedManifest> {
        ParsedManifest::parse(bytes)
    }

    /// Bytes [`Manifest::encode`] will produce, for sizing a buffer.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        let mut n = "janus/1\n".len()
            + "model=\n".len()
            + self.model.len()
            + "fw=\n".len()
            + self.firmware.len()
            + "chip=\n".len()
            + self.chip.tag().len();
        for d in self.declared {
            n += "cap=::\n".len()
                + d.capability.tag().len()
                + d.status.tag().len()
                + d.backing.len();
        }
        n
    }
}

/// The `model` / `firmware` field rule: non-empty, at most
/// [`MAX_FIELD_LEN`] bytes, no newline, no `=`, no control character.
fn check_field(field: &str) -> Result<()> {
    if field.is_empty()
        || field.len() > MAX_FIELD_LEN
        || field.bytes().any(|b| b == b'\n' || b == b'=' || b < 0x20)
    {
        return Err(Error::InvalidFormat);
    }
    Ok(())
}

/// The canonical lines, from any source of declarations: `find` answers for
/// each capability in [`Capability::ALL`] order, so the same facts always
/// produce the same bytes.
fn encode_lines<'s>(
    out: &mut [u8],
    model: &str,
    firmware: &str,
    chip: Chip,
    mut find: impl FnMut(Capability) -> Option<(Status, &'s str)>,
) -> Result<usize> {
    let mut w = Cursor::new(out);
    w.str("janus/")?;
    w.byte(b'0' + crate::FORMAT_VERSION)?;
    w.byte(b'\n')?;
    w.str("model=")?;
    w.str(model)?;
    w.byte(b'\n')?;
    w.str("fw=")?;
    w.str(firmware)?;
    w.byte(b'\n')?;
    w.str("chip=")?;
    w.str(chip.tag())?;
    w.byte(b'\n')?;
    for cap in Capability::ALL {
        if let Some((status, backing)) = find(*cap) {
            w.str("cap=")?;
            w.str(cap.tag())?;
            w.byte(b':')?;
            w.str(status.tag())?;
            w.byte(b':')?;
            w.str(backing)?;
            w.byte(b'\n')?;
        }
    }
    Ok(w.len())
}

/// One declaration read back from the wire (`alloc`).
#[cfg(feature = "alloc")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParsedDeclared {
    /// What.
    pub capability: Capability,
    /// How real.
    pub status: Status,
    /// The crate behind it (`""` when planned).
    pub backing: alloc::string::String,
}

#[cfg(feature = "alloc")]
impl ParsedDeclared {
    /// The honesty rule, as for [`Declared::validate`].
    pub fn validate(&self) -> Result<()> {
        let ok = match self.status {
            Status::Available | Status::Preview => !self.backing.is_empty(),
            Status::Planned => self.backing.is_empty(),
        };
        let clean = self
            .backing
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if ok && clean {
            Ok(())
        } else {
            Err(Error::InvalidFormat)
        }
    }
}

/// A manifest read back from its canonical encoding: what a host holds
/// after `janus/rpc/1` answered `Manifest`, or a bridge after a neighbour
/// sent its own. Owned, so it outlives the bytes it came from; `encode`
/// reproduces those bytes exactly, which is what lets the signature be
/// checked over what was parsed.
#[cfg(feature = "alloc")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedManifest {
    /// The maker's model string.
    pub model: alloc::string::String,
    /// Firmware version.
    pub firmware: alloc::string::String,
    /// The part.
    pub chip: Chip,
    /// Every declaration, in the order the wire had them.
    pub declared: alloc::vec::Vec<ParsedDeclared>,
}

#[cfg(feature = "alloc")]
impl ParsedManifest {
    /// Parse canonical bytes. `InvalidFormat` for anything that is not the
    /// canonical form (order, a missing trailing newline, a bad field, a
    /// duplicate capability, a status that is not one of the three);
    /// `Unsupported` for a format version or capability tag this crate does
    /// not know — a newer device, which a host must say it cannot read
    /// rather than read partially.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        use alloc::string::ToString;
        let text = core::str::from_utf8(bytes).map_err(|_| Error::InvalidFormat)?;
        let body = text.strip_suffix('\n').ok_or(Error::InvalidFormat)?;
        let mut lines = body.split('\n');
        match lines.next() {
            Some("janus/1") => {}
            Some(v) if v.starts_with("janus/") => return Err(Error::Unsupported),
            _ => return Err(Error::InvalidFormat),
        }
        let model = lines
            .next()
            .and_then(|l| l.strip_prefix("model="))
            .ok_or(Error::InvalidFormat)?;
        let firmware = lines
            .next()
            .and_then(|l| l.strip_prefix("fw="))
            .ok_or(Error::InvalidFormat)?;
        let chip_tag = lines
            .next()
            .and_then(|l| l.strip_prefix("chip="))
            .ok_or(Error::InvalidFormat)?;
        check_field(model)?;
        check_field(firmware)?;
        let chip = Chip::parse(chip_tag).ok_or(Error::Unsupported)?;
        let mut declared = alloc::vec::Vec::new();
        for line in lines {
            let rest = line.strip_prefix("cap=").ok_or(Error::InvalidFormat)?;
            let mut parts = rest.splitn(3, ':');
            let (tag, status, backing) = match (parts.next(), parts.next(), parts.next()) {
                (Some(t), Some(s), Some(b)) => (t, s, b),
                _ => return Err(Error::InvalidFormat),
            };
            let capability = Capability::parse(tag).ok_or(Error::Unsupported)?;
            let status = Status::parse(status).ok_or(Error::InvalidFormat)?;
            let d = ParsedDeclared {
                capability,
                status,
                backing: backing.to_string(),
            };
            d.validate()?;
            if declared
                .iter()
                .any(|e: &ParsedDeclared| e.capability == capability)
            {
                return Err(Error::InvalidFormat);
            }
            declared.push(d);
        }
        Ok(ParsedManifest {
            model: model.to_string(),
            firmware: firmware.to_string(),
            chip,
            declared,
        })
    }

    /// True when the manifest declares `capability` as available.
    #[must_use]
    pub fn has(&self, capability: Capability) -> bool {
        self.declared
            .iter()
            .any(|d| d.capability == capability && d.status == Status::Available)
    }

    /// The same rules as [`Manifest::validate`].
    pub fn validate(&self) -> Result<()> {
        check_field(&self.model)?;
        check_field(&self.firmware)?;
        for (i, d) in self.declared.iter().enumerate() {
            d.validate()?;
            if self.declared[..i]
                .iter()
                .any(|e| e.capability == d.capability)
            {
                return Err(Error::InvalidFormat);
            }
        }
        Ok(())
    }

    /// The canonical encoding, byte for byte what [`Manifest::encode`] writes
    /// for the same facts.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        self.validate()?;
        encode_lines(out, &self.model, &self.firmware, self.chip, |cap| {
            self.declared
                .iter()
                .find(|d| d.capability == cap)
                .map(|d| (d.status, d.backing.as_str()))
        })
    }

    /// Bytes [`ParsedManifest::encode`] will produce.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        let mut n = "janus/1\n".len()
            + "model=\n".len()
            + self.model.len()
            + "fw=\n".len()
            + self.firmware.len()
            + "chip=\n".len()
            + self.chip.tag().len();
        for d in &self.declared {
            n += "cap=::\n".len()
                + d.capability.tag().len()
                + d.status.tag().len()
                + d.backing.len();
        }
        n
    }

    /// The encoding as a fresh `Vec`.
    pub fn to_bytes(&self) -> Result<alloc::vec::Vec<u8>> {
        let mut out = alloc::vec![0u8; self.encoded_len()];
        let n = self.encode(&mut out)?;
        out.truncate(n);
        Ok(out)
    }
}

/// A bounds-checked byte writer over a caller buffer.
struct Cursor<'a> {
    out: &'a mut [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Cursor { out, pos: 0 }
    }

    fn len(&self) -> usize {
        self.pos
    }

    fn byte(&mut self, b: u8) -> Result<()> {
        match self.out.get_mut(self.pos) {
            Some(slot) => {
                *slot = b;
                self.pos += 1;
                Ok(())
            }
            None => Err(Error::BufferTooSmall {
                needed: self.pos + 1,
            }),
        }
    }

    fn str(&mut self, s: &str) -> Result<()> {
        let end = self.pos + s.len();
        match self.out.get_mut(self.pos..end) {
            Some(slot) => {
                slot.copy_from_slice(s.as_bytes());
                self.pos = end;
                Ok(())
            }
            None => Err(Error::BufferTooSmall { needed: end }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECLARED: &[Declared] = &[
        Declared::planned(Capability::IrohRelay),
        Declared::available(Capability::ImageJpeg, "rusty_esp_image"),
        Declared::preview(Capability::MidDevice, "rusty_esp_mid"),
    ];

    fn manifest() -> Manifest<'static> {
        Manifest {
            model: "acme/doorbell-2",
            firmware: "1.4.0",
            chip: Chip::Esp32S3,
            declared: DECLARED,
        }
    }

    #[test]
    fn tags_round_trip() {
        for c in Capability::ALL {
            assert_eq!(Capability::parse(c.tag()), Some(*c));
        }
        assert_eq!(Capability::parse("nope"), None);
        for s in [Status::Available, Status::Preview, Status::Planned] {
            assert_eq!(Status::parse(s.tag()), Some(s));
        }
    }

    #[test]
    fn honesty_rule() {
        assert!(Declared::available(Capability::Gpio, "x")
            .validate()
            .is_ok());
        assert!(Declared::available(Capability::Gpio, "")
            .validate()
            .is_err());
        assert!(Declared::planned(Capability::Gpio).validate().is_ok());
        assert!(Declared {
            capability: Capability::Gpio,
            status: Status::Planned,
            backing: "x",
        }
        .validate()
        .is_err());
        assert!(Declared::available(Capability::Gpio, "bad crate")
            .validate()
            .is_err());
    }

    #[test]
    fn encoding_is_canonical_and_sized() {
        let m = manifest();
        let mut buf = [0u8; 256];
        let n = m.encode(&mut buf).unwrap();
        assert_eq!(n, m.encoded_len());
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert_eq!(
            text,
            "janus/1\nmodel=acme/doorbell-2\nfw=1.4.0\nchip=esp32s3\ncap=image.jpeg:available:rusty_esp_image\ncap=iroh.relay:planned:\ncap=mid.device:preview:rusty_esp_mid\n"
        );
        assert!(m.has(Capability::ImageJpeg));
        assert!(!m.has(Capability::MidDevice));
        let mut small = [0u8; 16];
        assert!(matches!(
            m.encode(&mut small),
            Err(Error::BufferTooSmall { .. })
        ));
    }

    #[test]
    fn rejects_duplicates_and_bad_fields() {
        let dup = [
            Declared::planned(Capability::Gpio),
            Declared::planned(Capability::Gpio),
        ];
        let m = Manifest {
            declared: &dup,
            ..manifest()
        };
        assert_eq!(m.validate(), Err(Error::InvalidFormat));
        let m = Manifest {
            model: "bad=model",
            ..manifest()
        };
        assert_eq!(m.validate(), Err(Error::InvalidFormat));
    }

    #[test]
    fn chip_tags_parse_back() {
        for c in Chip::ALL {
            assert_eq!(Chip::parse(c.tag()), Some(*c));
        }
        assert_eq!(Chip::parse("esp8266"), None);
        assert_eq!(Chip::ALL.len(), 9);
    }

    #[cfg(feature = "alloc")]
    /// A small deterministic generator (an LCG) for the corpus below: no
    /// dependency, same corpus every run.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn field(&mut self, out: &mut alloc::string::String, max: usize) {
            const ALPHABET: &[u8] =
                b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/-_. :+";
            let len = 1 + self.below(max as u64) as usize;
            out.clear();
            for _ in 0..len {
                out.push(ALPHABET[self.below(ALPHABET.len() as u64) as usize] as char);
            }
        }
        fn backing(&mut self, out: &mut alloc::string::String) {
            const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_-";
            let len = 1 + self.below(24) as usize;
            out.clear();
            for _ in 0..len {
                out.push(ALPHABET[self.below(ALPHABET.len() as u64) as usize] as char);
            }
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn parse_round_trips_a_corpus_byte_for_byte() {
        let mut rng = Lcg(0x4A414E5553);
        let mut model = alloc::string::String::new();
        let mut firmware = alloc::string::String::new();
        let mut backing = alloc::string::String::new();
        let mut buf = [0u8; 4096];
        let mut again = [0u8; 4096];
        for _ in 0..400 {
            rng.field(&mut model, MAX_FIELD_LEN);
            rng.field(&mut firmware, MAX_FIELD_LEN);
            let chip = Chip::ALL[rng.below(Chip::ALL.len() as u64) as usize];
            let mut declared = alloc::vec::Vec::new();
            for cap in Capability::ALL {
                match rng.below(4) {
                    0 => {}
                    1 => declared.push(ParsedDeclared {
                        capability: *cap,
                        status: Status::Planned,
                        backing: alloc::string::String::new(),
                    }),
                    k => {
                        rng.backing(&mut backing);
                        declared.push(ParsedDeclared {
                            capability: *cap,
                            status: if k == 2 {
                                Status::Available
                            } else {
                                Status::Preview
                            },
                            backing: backing.clone(),
                        });
                    }
                }
            }
            // shuffle the declaration order: the encoding must not care
            for i in (1..declared.len()).rev() {
                let j = rng.below(i as u64 + 1) as usize;
                declared.swap(i, j);
            }
            let m = ParsedManifest {
                model: model.clone(),
                firmware: firmware.clone(),
                chip,
                declared,
            };
            let n = m.encode(&mut buf).unwrap();
            assert_eq!(n, m.encoded_len());
            let back = ParsedManifest::parse(&buf[..n]).unwrap();
            assert_eq!(back.model, m.model);
            assert_eq!(back.firmware, m.firmware);
            assert_eq!(back.chip, m.chip);
            assert_eq!(back.declared.len(), m.declared.len());
            let k = back.encode(&mut again).unwrap();
            assert_eq!(&again[..k], &buf[..n], "re-encoding is byte-identical");
            assert_eq!(back.to_bytes().unwrap(), &buf[..n]);
            for d in &m.declared {
                assert_eq!(back.has(d.capability), d.status == Status::Available);
            }
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn parse_agrees_with_the_borrowed_form_and_refuses_the_rest() {
        let m = manifest();
        let mut buf = [0u8; 512];
        let n = m.encode(&mut buf).unwrap();
        let p = Manifest::parse(&buf[..n]).unwrap();
        assert_eq!(p.model, m.model);
        assert_eq!(p.chip, m.chip);
        assert_eq!(p.declared.len(), m.declared.len());
        assert_eq!(p.to_bytes().unwrap(), &buf[..n]);
        // corruption never panics; the canonical form is the only form
        let mut rng = Lcg(7);
        for _ in 0..2000 {
            let mut bytes = buf[..n].to_vec();
            match rng.below(3) {
                0 => {
                    let i = rng.below(bytes.len() as u64) as usize;
                    bytes[i] = rng.below(256) as u8;
                }
                1 => {
                    let cut = rng.below(bytes.len() as u64) as usize;
                    bytes.truncate(cut);
                }
                _ => {
                    let i = rng.below(bytes.len() as u64) as usize;
                    bytes.insert(i, b'\n');
                }
            }
            if let Ok(again) = ParsedManifest::parse(&bytes) {
                // anything accepted must re-encode to what was accepted
                assert_eq!(again.to_bytes().unwrap(), bytes);
            }
        }
        let cases: [(&[u8], Error); 6] = [
            (b"janus/2\nmodel=a\nfw=1\nchip=esp32\n", Error::Unsupported),
            (
                b"janus/1\nmodel=a\nfw=1\nchip=esp8266\n",
                Error::Unsupported,
            ),
            (
                b"janus/1\nmodel=a\nfw=1\nchip=esp32\ncap=warp.drive:available:x\n",
                Error::Unsupported,
            ),
            (
                b"janus/1\nfw=1\nmodel=a\nchip=esp32\n",
                Error::InvalidFormat,
            ),
            (b"janus/1\nmodel=a\nfw=1\nchip=esp32", Error::InvalidFormat),
            (
                b"janus/1\nmodel=a\nfw=1\nchip=esp32\ncap=gpio:available:x\ncap=gpio:planned:\n",
                Error::InvalidFormat,
            ),
        ];
        for (bytes, err) in cases {
            assert_eq!(
                ParsedManifest::parse(bytes).err(),
                Some(err),
                "{:?}",
                core::str::from_utf8(bytes)
            );
        }
    }

    #[test]
    fn chip_facts() {
        assert!(!Chip::Esp32P4.has_wifi());
        assert!(Chip::Esp32P4.has_pie());
        assert!(Chip::Esp32C6.has_wifi());
        assert!(!Chip::Esp32C6.has_pie());
    }
}
