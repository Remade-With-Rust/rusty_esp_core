# rusty_esp_rtos

The Janus **RTOS seam**: the [Kairos](https://github.com/Remade-With-Rust/kairos)
kernel's port pinned once, with the `esp-hal` companion window it has to
agree with.

A Janus firmware that runs on `rusty_rtos` reaches it through this crate and
never names `rusty_rtos_port-*` itself — the same rule, for the same reason,
as `rusty_esp_alloc`. The pin and the compatibility window are decided here
once instead of in every firmware.

**It declares no scheduler and starts nothing.** It hands out the port and
states what that port requires; the firmware's own `main` does the starting,
exactly as it declares its own global allocator.

```toml
rusty_esp_rtos = { version = "0.1", features = ["port"] }
```

Nothing by default: a firmware that does not ask for the Kairos kernel gets
an empty crate, so adding this to a workspace changes no existing build.

## The joint, and why it is narrow

The Xtensa port runs its context switch inside a software interrupt so
`xtensa-lx-rt`'s exception entry spills the register windows. That makes
`xtensa-lx-rt` a hard joint with esp-hal — and it is a **`links` crate**, so
exactly one version may exist in a graph. The port and esp-hal agree or the
build fails outright, which is the good failure.

Checked 2026-09-20, read off resolved lockfiles rather than a plan:

| | xtensa-lx-rt |
|---|---|
| `rusty_rtos_port-xtensa` 0.1.0 (pinned to match esp-hal 1.2.1) | 0.23 |
| `janus/rusty_esp_dsp` `xiao-s3-probe` | **0.23.0** |
| `janus/rusty_esp_mid` `xiao-s3-keys` | **0.23.0** |

The joint already lines up.

## What it cannot do yet

Host the Wi-Fi/BLE blob. `esp-radio` reaches a scheduler through
`esp-radio-rtos-driver`, and the Kairos ports currently *document* that
interface without implementing it — that is Kairos's half of K5. Until it
lands, a Janus firmware with a radio keeps `esp-rtos`, and this seam carries
the pin so the switch is one feature flag rather than six manifests.

`compat::HOSTS_ESP_RADIO` says so in code rather than by omission, because
"not measurable on this target" and "measured false" are different states.

The integration contract, written for the Kairos side, is
`docs/plans/janus-rtos.md` in the Kairos umbrella.
