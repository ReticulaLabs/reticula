//! Minimal NMEA reader for the T-Deck's on-board MIA-M10Q GNSS.
//!
//! The module exists for one reason: to give the device a wall clock when it
//! is away from WiFi. See [`crate::clock`] for why that matters — an announce
//! stamped with a 1970 timestamp is discarded by every peer, so a LoRa node
//! with no clock is invisible to the network.
//!
//! # Why not a full NMEA stack
//!
//! We need one field from one sentence. The GNSS defaults to 9600 8N1 and
//! emits a standard NMEA stream; the shortest way to a UTC time is to scan for
//! an `$GPRMC`/`$GNRMC` (or `$GPZDA`/`$GNZDA`) sentence and parse the date and
//! time fields out of it. Both are fixed-format, checksummed, and arrive once a
//! second, which is plenty.
//!
//! # Caveats
//!
//! * A time is only meaningful once the receiver has a fix. Indoors this may
//!   never happen, so the reader simply never fires and the node falls back to
//!   SNTP or the mesh clock. That is expected, not an error.
//! * A cold start with no almanac can take minutes. The reader is therefore
//!   non-blocking: it runs on its own task and sets the clock whenever a fix
//!   turns up, so it keeps working if the device is carried outside later.
//! * The time is UTC. `set_system_time` enforces the only-forward rule, so a
//!   bad sentence can never push the clock backwards.

use esp_idf_hal::delay::TickType;
use esp_idf_hal::gpio::{Gpio43, Gpio44};
use esp_idf_hal::uart::{self, UartDriver, UART1};

use crate::clock::{self, Source};

/// MIA-M10Q factory default baud. Some firmware revisions ship at 115200; if no
/// fix turns up, this is the first thing to try.
const GNSS_BAUD: u32 = 9600;

/// Bytes of NMEA we keep while looking for a sentence boundary. NMEA sentences
/// are capped at 82 characters, so this is generous.
const SENTENCE_BUF: usize = 96;

/// Open the GNSS's UART and return a driver for it, or `None` on failure.
///
/// Call this from the thread that owns the peripherals, then hand the driver
/// to [`spawn`]; it must be opened before the pins are moved elsewhere.
pub fn open(uart1: UART1, tx: Gpio43, rx: Gpio44) -> Option<UartDriver<'static>> {
    // LilyGO T-Deck pin mapping: GPIO43 is MCU TX (to GNSS RX), GPIO44 is
    // MCU RX (from GNSS TX). UART0 is reserved for the console, so the GNSS
    // goes on UART1.
    //
    // No flow control: the GNSS module drives only TX, and its one-way NMEA
    // rate is far below the UART's capacity.
    let driver: UartDriver<'_> = match UartDriver::new(
        uart1,
        tx,
        rx,
        None::<Gpio43>,
        None::<Gpio43>,
        &uart::config::Config::default().baudrate(esp_idf_hal::units::Hertz(GNSS_BAUD)),
    ) {
        Ok(driver) => driver,
        Err(e) => {
            log::warn!("gnss: could not open UART1 on GPIO43/44: {e}");
            return None;
        }
    };

    log::info!("gnss: listening on UART1 GPIO43/44 at {GNSS_BAUD} baud");

    // The driver borrows nothing (`'d` is a PhantomData lifetime), so it can
    // outlive this call and be moved onto the reader thread.
    // SAFETY: `UartDriver<'d>` holds only a port number, an optional queue
    // handle and a `PhantomData<&'d mut ()>`. It never dereferences `'d`, so
    // widening it to `'static` is sound and lets the driver move to a reader
    // thread. The driver owns the UART for as long as it lives regardless.
    let driver: UartDriver<'static> = unsafe { core::mem::transmute(driver) };
    Some(driver)
}

/// Read time from the GNSS, and apply it via [`clock::set_system_time`].
///
/// Runs until the process ends. Failures are logged at debug level rather than
/// warned about, because "no fix" is the normal case indoors and should not
/// read like a fault.
pub fn spawn(driver: UartDriver<'static>) {
    let mut sentence = [0u8; SENTENCE_BUF];
    let mut filled = 0usize;

    loop {
        let mut byte = [0u8; 1];
        // Bound the wait so the task stays responsive and the read loop keeps
        // draining the driver's buffer.
        let n = match driver.read(&mut byte, TickType::new_millis(200).0) {
            Ok(n) => n,
            Err(e) => {
                log::debug!("gnss: read error: {e}");
                continue;
            }
        };
        if n == 0 {
            continue; // idle tick; keep waiting for a fix
        }

        // '$' starts a sentence; anything else resets the buffer. Cheap, and
        // it resynchronises us after a dropped byte without a real parser.
        if byte[0] == b'$' {
            filled = 0;
        }
        if filled >= SENTENCE_BUF {
            filled = 0;
        }
        sentence[filled] = byte[0];
        filled += 1;

        // Sentences terminate with CR/LF; try to parse on either.
        if byte[0] != b'\r' && byte[0] != b'\n' {
            continue;
        }

        let text = match core::str::from_utf8(&sentence[..filled]) {
            Ok(text) => text,
            Err(_) => {
                filled = 0;
                continue;
            }
        };
        filled = 0;

        if let Some(unix) = parse_sentence_time(text) {
            log::info!("gnss: fix, time of week decoded as unix {unix}");
            if clock::set_system_time(unix, Source::Gnss) {
                clock::log_status("after GNSS fix");
                // Keep reading: later fixes correct any drift, and
                // `set_system_time` refuses to move the clock backwards.
            }
        }
    }
}

/// Extract a UTC timestamp from an NMEA sentence, if it carries a usable one.
///
/// Handles `$__RMC` (recommended minimum, which carries both date and time)
/// and `$__ZDA`. Any talker id is accepted (`$GP`, `$GN`, …), and the checksum
/// is verified when present — a sentence we can't validate is a sentence we
/// shouldn't be taking a clock from.
fn parse_sentence_time(sentence: &str) -> Option<u64> {
    let (body, checksum) = split_checksum(sentence)?;
    if !checksum_valid(body, checksum) {
        return None;
    }

    let fields: Vec<&str> = body.split(',').collect();

    match fields[0] {
        // $GPRMC,time,status,lat,N,lon,E,speed,course,date,magvar,...
        "GPRMC" | "GNRMC" | "GLRMC" | "GARMC" => {
            // A receiver with no fix reports status 'V'; its time is void.
            if fields.get(2) != Some(&"A") {
                return None;
            }
            datetime_to_unix(fields.get(9)?, fields.get(1)?)
        }
        // $GPZDA,time,day,month,year,localhour,localminute
        "GPZDA" | "GNZDA" | "GLZDA" | "GAZDA" => {
            let day: u32 = fields.get(2)?.parse().ok()?;
            let month: u32 = fields.get(3)?.parse().ok()?;
            let year: u32 = fields.get(4)?.parse().ok()?;
            let hhmmss = hhmmss(fields.get(1)?)?;
            unix_from_parts(year, month, day, hhmmss)
        }
        _ => None,
    }
}

/// Split `$<body>*<checksum>` into the two halves.
///
/// The trailing `*` and its two hex digits are optional, because some modules
/// emit sentences without them; [`checksum_valid`] copes with that.
fn split_checksum(sentence: &str) -> Option<(&str, Option<&str>)> {
    let sentence = sentence.trim();
    let body = sentence.strip_prefix('$')?;
    match body.split_once('*') {
        Some((body, checksum)) => Some((body, Some(checksum))),
        None => Some((body, None)),
    }
}

/// Verify the NMEA XOR checksum over `body`, if the sentence carries one.
fn checksum_valid(body: &str, checksum: Option<&str>) -> bool {
    let Some(expected) = checksum else {
        return true; // no checksum supplied, nothing to check
    };
    let expected = expected.trim();
    if expected.len() != 2 {
        return false;
    }
    let Ok(expected) = u8::from_str_radix(expected, 16) else {
        return false;
    };

    let actual = body
        .as_bytes()
        .iter()
        .fold(0u8, |acc, b| acc ^ b);

    actual == expected
}

/// `hhmmss` or `hhmmss.ss` → seconds since midnight, range-checked.
fn hhmmss(field: &str) -> Option<u64> {
    let whole = field.split('.').next()?;
    if whole.len() != 6 || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let secs: u64 = whole.parse().ok()?;
    if secs > 235_959 {
        return None;
    }
    Some(secs)
}

/// `ddmmyy` + `hhmmss(.ss)` → Unix seconds.
fn datetime_to_unix(date: &str, time: &str) -> Option<u64> {
    if date.len() != 6 || !date.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let day: u32 = date[0..2].parse().ok()?;
    let month: u32 = date[2..4].parse().ok()?;
    let yy: u32 = date[4..6].parse().ok()?;
    // The NMEA two-digit year pivots at 1980 (the GPS epoch), so 80..99 is
    // 20th century and 00..79 is 21st.
    let year = if yy >= 80 { 1900 + yy } else { 2000 + yy };

    unix_from_parts(year, month, day, hhmmss(time)?)
}

/// Combine a civil date and an `hhmmss`-style time-of-day into Unix seconds.
///
/// All arithmetic goes through `i64` so the sign is handled in one place; the
/// caller only ever sees a `u64`, and every input is range-checked before it
/// gets here.
fn unix_from_parts(year: u32, month: u32, day: u32, hhmmss: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let secs_of_day = (hhmmss / 10_000) * 3600 + ((hhmmss / 100) % 100) * 60 + (hhmmss % 100);
    let secs = days_from_civil(year, month, day)
        .checked_mul(86_400)?
        .checked_add(secs_of_day as i64)?;
    u64::try_from(secs).ok()
}

/// Days between 1970-01-01 and the given civil date (Howard Hinnant's
/// `days_from_civil`; proleptic Gregorian, valid for any year).
///
/// Returns days since the Unix epoch, which is what callers need — no GNSS
/// epoch arithmetic is involved.
fn days_from_civil(y: u32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = m as i64;
    let d = d as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2024-01-02 09:15:42 UTC.
    const EXPECTED: u64 = 1_704_186_942;

    /// XOR of the bytes between `$` and `*`, as two hex digits.
    fn checksum(body: &str) -> String {
        format!(
            "{:02X}",
            body.as_bytes().iter().fold(0u8, |acc, b| acc ^ b)
        )
    }

    fn sentence(body: &str) -> String {
        format!("${body}*{}", checksum(body))
    }

    const RMC_BODY: &str =
        "GNRMC,091542.00,A,5107.03815,N,01200.69829,E,0.004,84.4,020124,,,A";
    const ZDA_BODY: &str = "GNZDA,091542.00,02,01,2024,00,00";

    #[test]
    fn rmc_yields_unix_time() {
        assert_eq!(parse_sentence_time(&sentence(RMC_BODY)), Some(EXPECTED));
    }

    #[test]
    fn zda_yields_unix_time() {
        assert_eq!(parse_sentence_time(&sentence(ZDA_BODY)), Some(EXPECTED));
    }

    #[test]
    fn both_sentences_agree() {
        // RMC and ZDA describe the same instant, so they must decode alike —
        // this catches a field-index mistake in either parser.
        assert_eq!(
            parse_sentence_time(&sentence(RMC_BODY)),
            parse_sentence_time(&sentence(ZDA_BODY)),
        );
    }

    #[test]
    fn void_rmc_is_ignored() {
        // Status 'V' — receiver has no fix, so its time field is meaningless.
        let body = "GNRMC,,V,,,,,,,,,,N";
        assert_eq!(parse_sentence_time(&sentence(body)), None);
    }

    #[test]
    fn a_bad_checksum_is_rejected() {
        let mut s = sentence(RMC_BODY);
        s.pop(); // drop the real checksum digit
        s.push('0');
        assert_eq!(parse_sentence_time(&s), None);
    }

    #[test]
    fn a_missing_checksum_is_tolerated() {
        // Some modules emit sentences without one; don't throw away a good fix.
        assert_eq!(parse_sentence_time(&format!("${RMC_BODY}")), Some(EXPECTED));
    }

    #[test]
    fn unrelated_sentences_are_ignored() {
        assert_eq!(parse_sentence_time("garbage"), None);
        assert_eq!(parse_sentence_time(""), None);
        assert_eq!(parse_sentence_time("$GNVTG,,T,,,M,,N,,M*4B"), None);
        // GGA carries no date, so it can't give us a clock.
        assert_eq!(parse_sentence_time("$GNGGA,091542.00,,,,,0,00,25.0,,,,,*4A"), None);
        // Truncated RMC: no date field at all.
        assert_eq!(parse_sentence_time("$GNRMC,091542.00,A"), None);
    }

    #[test]
    fn out_of_range_fields_are_rejected() {
        // Month 13, day 32, hour 25.
        assert_eq!(parse_sentence_time(&sentence("GNZDA,091542.00,02,13,2024,00,00")), None);
        assert_eq!(parse_sentence_time(&sentence("GNZDA,091542.00,32,01,2024,00,00")), None);
        assert_eq!(parse_sentence_time(&sentence("GNZDA,250000.00,02,01,2024,00,00")), None);
        // Non-numeric where digits are required.
        assert_eq!(parse_sentence_time(&sentence("GNZDA,0915x2.00,02,01,2024,00,00")), None);
    }

    #[test]
    fn civil_conversion_matches_known_epochs() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 1, 1), 10_957);
        assert_eq!(days_from_civil(2024, 1, 2), 19_724);
        // 2024-01-02 00:00:00Z
        assert_eq!(days_from_civil(2024, 1, 2) * 86_400, 1_704_153_600);
    }

    #[test]
    fn two_digit_year_pivots_at_1980() {
        // 85 -> 1985, 24 -> 2024
        assert!(datetime_to_unix("020185", "000000").unwrap() < 500_000_000);
        assert!(datetime_to_unix("020124", "000000").unwrap() > 1_700_000_000);
    }

    #[test]
    fn leap_day_is_handled() {
        // 2024 is a leap year, so 29 Feb exists and follows 28 Feb by a day.
        assert_eq!(
            datetime_to_unix("290224", "120000").unwrap()
                - datetime_to_unix("280224", "120000").unwrap(),
            86_400
        );
        // And 1 Mar follows it, which only holds in a leap year.
        assert_eq!(
            datetime_to_unix("010324", "000000").unwrap()
                - datetime_to_unix("290224", "000000").unwrap(),
            86_400
        );
    }
}