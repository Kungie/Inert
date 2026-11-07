//! Local dates and times for the history page (no time-zone database: the
//! system's `localtime_r` does the conversion).

pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub wday: u32,
}

pub fn local(t: u64) -> Local {
    let tt = t as libc::time_t;
    // SAFETY: localtime_r only writes into the `tm` we pass it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&tt, &mut tm) };
    Local { year: tm.tm_year + 1900, month: tm.tm_mon as u32 + 1, day: tm.tm_mday as u32, hour: tm.tm_hour as u32, min: tm.tm_min as u32, wday: tm.tm_wday as u32 }
}

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// A key that is equal for two times on the same local day.
pub fn day_key(t: u64) -> (i32, u32, u32) {
    let l = local(t);
    (l.year, l.month, l.day)
}

/// "Today — Saturday, 7 November 2026", "Yesterday — …", or the date.
pub fn day_label(t: u64, now: u64) -> String {
    let l = local(t);
    let date = format!("{}, {} {} {}", DAYS[l.wday as usize % 7], l.day, MONTHS[(l.month as usize + 11) % 12], l.year);
    if day_key(t) == day_key(now) {
        format!("Today — {date}")
    } else if day_key(t) == day_key(now.saturating_sub(86_400)) {
        format!("Yesterday — {date}")
    } else {
        date
    }
}

pub fn clock(t: u64) -> String {
    let l = local(t);
    format!("{:02}:{:02}", l.hour, l.min)
}

/// "7 Nov 2026".
pub fn date(t: u64) -> String {
    let l = local(t);
    format!("{} {} {}", l.day, &MONTHS[(l.month as usize + 11) % 12][..3], l.year)
}
