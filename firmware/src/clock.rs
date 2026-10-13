//! Wall-clock recovery for the T-Deck.
//!
//! The ESP32-S3 has no RTC that survives a power cycle, so on boot the system
//! clock starts at 0 and only counts up from boot until something syncs it.
//! That number is used verbatim as the Reticulum announce timestamp (the SDK
//! reads `UNIX_EPOCH.elapsed()` in `destination.rs`), and peers compare an
//! announce's timebase against the one they already hold for that destination.
//! An unsynced node therefore announces as 1970, every peer discards it as
//! stale, and it never appears on the network — and it loses that again on
//! every reboot, because the clock resets and the timebase goes backwards.
//!
//! So the clock matters even when nothing else does. There are three ways to
//! get one, and they are independent of each other:
//!
//! * **SNTP** over WiFi (`start_sntp`) — the best source when the device is
//!   stationary and has a network, and needs no sky view.
//! * **GNSS** (`gps::spawn`) — the best source when the device is on the move,
//!   and needs no network. Needs a sky view, and a fix takes seconds to
//!   minutes.
//! * **The Reticulum mesh itself** — every announce carries its sender's
//!   timestamp, so a node that can hear the network can agree with it. See
//!   the SDK's mesh-clock work; it only engages while this clock is implausible.
//!
//! All of them funnel through [`set_system_time`], which enforces the two
//! invariants that keep the network working: never accept an implausible time,
//! and never move the clock backwards.

use esp_idf_sys::{settimeofday, timeval, timezone};

/// Unix time below which the system clock is certainly unset.
///
/// This is the 2020-09-13 threshold; anything below it is not a real time, and
/// an announce stamped with it will be rejected by every peer.
pub const PLAUSIBLE_UNIX_TIME: u64 = 1_600_000_000;

/// Where a recovered time came from, for logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// GNSS (the T-Deck's MIA-M10Q).
    Gnss,
}

impl core::fmt::Display for Source {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Source::Gnss => "GNSS",
        })
    }
}

/// Seconds since the Unix epoch, 0 if the clock is before it.
pub fn unix_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Set the system clock to `secs`, if that is an improvement.
///
/// Returns whether the clock was changed. Two things are refused:
///
/// * **Implausible times.** A source that reports something before
///   [`PLAUSIBLE_UNIX_TIME`] is broken, not merely early, and trusting it
///   would put us back on the announce blacklist we are trying to escape.
/// * **Times in the past.** Announce freshness is `emitted > stored`, so a
///   backwards step guarantees the next announce is rejected as stale — which
///   is precisely the reboot failure this module exists to stop. Once the
///   clock is set, only corrections *forward* are allowed.
pub fn set_system_time(secs: u64, source: Source) -> bool {
    let current = unix_time_secs();

    if secs < PLAUSIBLE_UNIX_TIME {
        log::warn!(
            "{source}: ignoring implausible time {secs} (before {PLAUSIBLE_UNIX_TIME})"
        );
        return false;
    }

    if current >= PLAUSIBLE_UNIX_TIME && secs < current {
        log::warn!(
            "{source}: ignoring time {secs}, which is behind the current clock {current} — \
             moving the clock backwards would make our announces look stale to peers"
        );
        return false;
    }

    let tv = timeval {
        tv_sec: secs as esp_idf_sys::time_t,
        tv_usec: 0,
    };
    // SAFETY: `tv` is a valid, initialised `timeval`, and `timezone` is only
    // consulted when it is non-null, so a null pointer is explicitly allowed.
    let rc = unsafe { settimeofday(&tv, core::ptr::null::<timezone>()) };

    if rc != 0 {
        log::warn!("{source}: settimeofday({secs}) failed with {rc}");
        return false;
    }

    log::info!(
        "{source}: system clock set to {secs} (was {current}{})",
        if current < PLAUSIBLE_UNIX_TIME {
            ", unset"
        } else {
            ""
        }
    );
    true
}

/// Report whether the clock looks usable, so a node that is silently invisible
/// to the network says so instead of just looking quiet.
pub fn log_status(when: &str) {
    let now = unix_time_secs();
    if now < PLAUSIBLE_UNIX_TIME {
        log::warn!(
            "{when}: system clock is unset ({now}s since epoch) — announces will be \
             stamped ~1970 and every peer will discard them as stale, leaving this \
             node invisible to the network. Enable WiFi, or take the device outside \
             so the GNSS can see the sky."
        );
    } else {
        log::info!("{when}: system clock is {now} (plausible)");
    }
}

/// Start SNTP and report each sync. The client is leaked into a static so it
/// keeps running for the lifetime of the process, which is what ESP-IDF's SNTP
/// service needs (the C bindings only hold a pointer to it).
///
/// ESP-IDF's SNTP client sets the clock itself, so there is nothing to hand to
/// [`set_system_time`] here — we just read the result back and report it.
pub fn start_sntp() {
    let result = esp_idf_svc::sntp::EspSntp::new_with_callback(&Default::default(), |interval| {
        log::info!("SNTP synced, next sync in {interval:?}");
        // ESP-IDF's SNTP client sets the clock itself, so read it back rather
        // than duplicating what it did.
        log_status("after SNTP sync");
    });

    match result {
        Ok(sntp) => {
            log::info!("SNTP client started");
            Box::leak(Box::new(sntp));
        }
        Err(e) => log::warn!("SNTP client not started: {e}"),
    }
}