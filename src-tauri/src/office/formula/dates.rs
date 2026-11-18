//! Dates (serial numbers: days since 1899-12-30, or since 1904-01-01)

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// Unix day number of serial 0
fn epoch(date1904: bool) -> i64 {
    if date1904 { days_from_civil(1904, 1, 1) } else { days_from_civil(1899, 12, 30) }
}

pub(super) fn date_serial(y: i64, m: i64, d: i64, date1904: bool) -> Option<f64> {
    // years 0-1899 count from 1900, as in Excel
    let y = if (0..1900).contains(&y) { y + 1900 } else { y };
    let total = y * 12 + (m - 1);
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let serial = days_from_civil(y, m, 1) + d - 1 - epoch(date1904);
    (serial >= 0).then_some(serial as f64)
}

/// The serial number of a date (1900 date system)
pub fn serial(y: i64, m: i64, d: i64) -> Option<f64> {
    date_serial(y, m, d, false)
}

pub(super) fn serial_date(serial: f64, date1904: bool) -> Option<(i64, i64, i64)> {
    if !(0.0..3e6).contains(&serial) {
        return None;
    }
    Some(civil_from_days(serial.floor() as i64 + epoch(date1904)))
}

pub(super) fn days_in_month(y: i64, m: i64) -> i64 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)
}

pub fn today_serial(date1904: bool) -> Option<f64> {
    Some(today(date1904))
}

/// (year, month) of a serial date
pub fn year_month(serial: f64, date1904: bool) -> Option<(i64, i64)> {
    serial_date(serial, date1904).map(|(y, m, _)| (y, m))
}

/// Day of the week of a serial date, 0 = Sunday
pub fn weekday0(serial: f64, date1904: bool) -> i64 {
    (serial.floor() as i64 + if date1904 { 1462 } else { 0 } - 1).rem_euclid(7)
}

pub(super) fn today(date1904: bool) -> f64 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    secs / 86400.0 - epoch(date1904) as f64
}
