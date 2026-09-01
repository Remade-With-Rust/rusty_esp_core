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
        if self.model.is_empty()
            || self.model.len() > MAX_FIELD_LEN
            || self.firmware.is_empty()
            || self.firmware.len() > MAX_FIELD_LEN
        {
            return Err(Error::InvalidFormat);
        }
        for field in [self.model, self.firmware] {
            if field.bytes().any(|b| b == b'\n' || b == b'=' || b < 0x20) {
                return Err(Error::InvalidFormat);
            }
        }
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
        let mut w = Cursor::new(out);
        w.str("janus/")?;
        w.byte(b'0' + crate::FORMAT_VERSION)?;
        w.byte(b'\n')?;
        w.str("model=")?;
        w.str(self.model)?;
        w.byte(b'\n')?;
        w.str("fw=")?;
        w.str(self.firmware)?;
        w.byte(b'\n')?;
        w.str("chip=")?;
        w.str(self.chip.tag())?;
        w.byte(b'\n')?;
        for cap in Capability::ALL {
            if let Some(d) = self.declared.iter().find(|d| d.capability == *cap) {
                w.str("cap=")?;
                w.str(cap.tag())?;
                w.byte(b':')?;
                w.str(d.status.tag())?;
                w.byte(b':')?;
                w.str(d.backing)?;
                w.byte(b'\n')?;
            }
        }
        Ok(w.len())
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
        assert!(
            Declared::available(Capability::Gpio, "x")
                .validate()
                .is_ok()
        );
        assert!(
            Declared::available(Capability::Gpio, "")
                .validate()
                .is_err()
        );
        assert!(Declared::planned(Capability::Gpio).validate().is_ok());
        assert!(
            Declared {
                capability: Capability::Gpio,
                status: Status::Planned,
                backing: "x",
            }
            .validate()
            .is_err()
        );
        assert!(
            Declared::available(Capability::Gpio, "bad crate")
                .validate()
                .is_err()
        );
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
    fn chip_facts() {
        assert!(!Chip::Esp32P4.has_wifi());
        assert!(Chip::Esp32P4.has_pie());
        assert!(Chip::Esp32C6.has_wifi());
        assert!(!Chip::Esp32C6.has_pie());
    }
}
