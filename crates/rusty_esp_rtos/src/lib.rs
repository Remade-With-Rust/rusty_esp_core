#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
//! The RTOS seam: the Kairos kernel's port, pinned once for the family.
//!
//! Every Janus firmware that runs on `rusty_rtos` reaches it through this
//! crate and never names `rusty_rtos_port-*` itself. Same rule, and same
//! reason, as [`rusty_esp_alloc`](https://docs.rs/rusty_esp_alloc): the pin,
//! the compatibility window and the startup shape are decided here once
//! rather than in every firmware.
//!
//! **This crate declares no scheduler and starts nothing.** It hands out the
//! port and states what that port requires; the deliverable's own `main`
//! does the starting, exactly as it declares its own global allocator.
//!
//! # The window, and why it is narrow
//!
//! The Xtensa port's context switch runs inside a software interrupt so that
//! `xtensa-lx-rt`'s exception entry spills the register windows — which is
//! how FreeRTOS and `esp-rtos` do it, and what a hand-written spill from task
//! context could not do. That makes `xtensa-lx-rt` a hard joint between the
//! port and esp-hal, and it is a **`links` crate**: exactly one version may
//! exist in a dependency graph, so the port and esp-hal must agree or the
//! build fails outright rather than misbehaving.
//!
//! Checked on 2026-09-20, and the news is good:
//!
//! ```text
//!   rusty_rtos_port-xtensa 0.1.0  pins xtensa-lx-rt 0.23  (to match esp-hal 1.2.1)
//!   janus xiao-s3-probe           resolves xtensa-lx-rt 0.23.0
//!   janus xiao-s3-keys            resolves xtensa-lx-rt 0.23.0
//! ```
//!
//! The joint already lines up. See [`compat`] for the rest of the window.
//!
//! # What this cannot do yet
//!
//! Host the Wi-Fi/BLE blob. `esp-radio` reaches a scheduler through the
//! `esp-radio-rtos-driver` interface, and as of 2026-09-20 the Kairos ports
//! **document** that interface but do not implement it — it is the Kairos
//! half of K5. Until then a Janus firmware with a radio keeps `esp-rtos`,
//! and this seam carries the pin so that the day the ports implement it, the
//! change on our side is one feature flag rather than six manifests.
//!
//! The integration contract, written for the Kairos side, is
//! `docs/plans/janus-rtos.md` in the Kairos umbrella.

/// The compatibility window this seam guarantees, as facts a build can check
/// against rather than prose someone has to remember.
///
/// Every value here was read off a resolved lockfile or a published
/// manifest, not from a plan.
pub mod compat {
    /// The `esp-hal` versions `esp-rtos` 0.4 accepts (`~1.2.0-rc.0`), which
    /// is the set a Janus firmware may pin while remaining portable to the
    /// Kairos kernel.
    pub const ESP_HAL: &[&str] = &["1.2.0", "1.2.1", "1.2.2"];

    /// The `xtensa-lx-rt` version the Xtensa port and esp-hal must share.
    ///
    /// A `links` crate: one version per graph. If a firmware ever resolves
    /// something else, the port cannot go under it, and cargo will say so
    /// loudly rather than producing a subtly wrong binary.
    pub const XTENSA_LX_RT: &str = "0.23";

    /// The `esp-radio-rtos-driver` interface the Kairos ports will implement
    /// so `esp-radio`'s blob can run on the Kairos scheduler.
    ///
    /// Not implemented yet — see the crate docs.
    pub const ESP_RADIO_RTOS_DRIVER: &str = "0.4";

    /// Whether the port for THIS target can currently host `esp-radio`.
    ///
    /// Always `false` today. It is a constant rather than an omission
    /// because "not measurable on this target" and "measured false" are
    /// different states, and a seam that stays silent about which one it is
    /// reports success by accident.
    pub const HOSTS_ESP_RADIO: bool = false;
}

/// The Kairos port for the target being built, or `None` where there is
/// none — a host build, or an architecture Kairos has not ported.
///
/// This is what a firmware prints at startup to prove which kernel it is on,
/// the same way the probe firmwares print their allocator arm. A capability
/// you cannot detect is one you must not claim.
#[must_use]
pub const fn port_name() -> Option<&'static str> {
    #[cfg(all(feature = "port", target_arch = "xtensa"))]
    {
        Some("rusty_rtos_port-xtensa")
    }
    #[cfg(all(feature = "port", target_arch = "riscv32"))]
    {
        Some("rusty_rtos_port-riscv")
    }
    #[cfg(not(all(feature = "port", any(target_arch = "xtensa", target_arch = "riscv32"))))]
    {
        None
    }
}

/// The Kairos port itself, re-exported so a firmware names this crate and
/// not the port.
#[cfg(all(feature = "port", target_arch = "xtensa"))]
pub use rusty_rtos_port_xtensa as port;

/// The Kairos port itself, re-exported so a firmware names this crate and
/// not the port.
#[cfg(all(feature = "port", target_arch = "riscv32"))]
pub use rusty_rtos_port_riscv as port;

/// The Kairos shared vocabulary — ticks, priorities, handles, the `Config`
/// trait — re-exported for the same reason.
#[cfg(feature = "port")]
pub use rusty_rtos_core as core_types;

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam must compile and answer honestly on a host, where there is
    /// no port. A crate that only works on the chip cannot be tested in CI,
    /// and an untested seam is where the drift starts.
    #[test]
    fn a_host_build_claims_no_port() {
        assert_eq!(port_name(), None);
        assert!(!compat::HOSTS_ESP_RADIO);
    }

    /// The window is stated, not implied.
    #[test]
    fn the_window_is_not_empty() {
        assert!(compat::ESP_HAL.contains(&"1.2.0"));
        assert_eq!(compat::XTENSA_LX_RT, "0.23");
    }
}
