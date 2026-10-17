# NVS fixture images

Read back by `src/nvs.rs`'s tests, key for key. Written by `espino-nvs`
(`cargo run -p espino-nvs --example fixtures -- <this directory>` in the
espino repository), whose output is byte-identical to Espressif's
`nvs_partition_gen.py` — which is what makes these an oracle rather than a
round trip through our own code.

| file | partition | holds |
|---|---:|---|
| `janus-settings.bin` | 24 KB | the `janus` namespace an owner provisions: `name`, `wifi.ssid`, `wifi.psk` (`census-pass`, a placeholder), `maker`, `blink_ms` (u32), `fps` (u8) |
| `identity-blob.bin` | 12 KB | `janus/mid.devkey`, a 32-byte blob (`0..32`) — the shape of a device key; also what our writer must produce byte for byte from a blank partition |
| `mixed.bin` | 16 KB | two namespaces, a `u8`, an `i32`, a string, and a 5,000-byte blob that spans two pages |
