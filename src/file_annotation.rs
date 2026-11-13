//! Annotations for `find-file` / `write-file` candidates.
//!
//! Mirrors `marginalia-annotate-file` with `marginalia-align` at its default
//! `left`: file modes, human readable size, modification time and — only when
//! it differs from the current process — owner. Attributes come from `lstat`
//! like `file-attributes`, so a symlink is annotated as a link.

use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `marginalia-max-relative-age`: older files show an absolute time.
const MAX_RELATIVE_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 14);
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `marginalia--fields` joins non-left fields with `marginalia-separator`.
const FIELD_SEPARATOR: &str = "  ";

/// Passwd and group names already resolved while annotating one listing, so a
/// directory of foreign-owned files does not repeat the same lookups.
#[derive(Default)]
pub struct OwnerNames {
    users: std::collections::HashMap<u32, String>,
    groups: std::collections::HashMap<u32, String>,
}

impl OwnerNames {
    fn user(&mut self, uid: u32) -> &str {
        self.users
            .entry(uid)
            .or_insert_with(|| user_name(uid).unwrap_or_else(|| uid.to_string()))
    }

    fn group(&mut self, gid: u32) -> &str {
        self.groups
            .entry(gid)
            .or_insert_with(|| group_name(gid).unwrap_or_else(|| gid.to_string()))
    }
}

/// Annotation text for a directory entry: modes, `%7s` size and `%12s` time,
/// then the owner when it is not the current user and group. Like
/// `marginalia--fields`, an empty owner field keeps its separator.
pub fn file(metadata: &fs::Metadata, now: SystemTime, names: &mut OwnerNames) -> String {
    format!(
        "{}  {:>7}  {:>12}{FIELD_SEPARATOR}{}",
        modes(metadata),
        size(metadata.len()),
        time(metadata.modified().ok(), now),
        owner(metadata, names).unwrap_or_default()
    )
}

/// `file-attribute-modes`: the ten character `ls -l` mode string.
#[cfg(unix)]
fn modes(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};

    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else if file_type.is_block_device() {
        'b'
    } else if file_type.is_char_device() {
        'c'
    } else if file_type.is_fifo() {
        'p'
    } else if file_type.is_socket() {
        's'
    } else {
        '-'
    };
    let mode = metadata.permissions().mode();
    let mut text = String::with_capacity(10);
    text.push(kind);
    for (index, shift) in [6, 3, 0].into_iter().enumerate() {
        let bits = (mode >> shift) & 7;
        text.push(if bits & 4 != 0 { 'r' } else { '-' });
        text.push(if bits & 2 != 0 { 'w' } else { '-' });
        let special = match index {
            0 => mode & 0o4000 != 0,
            1 => mode & 0o2000 != 0,
            _ => mode & 0o1000 != 0,
        };
        text.push(match (bits & 1 != 0, special, index) {
            (true, false, _) => 'x',
            (false, false, _) => '-',
            (true, true, 2) => 't',
            (false, true, 2) => 'T',
            (true, true, _) => 's',
            (false, true, _) => 'S',
        });
    }
    text
}

#[cfg(not(unix))]
fn modes(metadata: &fs::Metadata) -> String {
    let kind = if metadata.is_dir() { 'd' } else { '-' };
    let permissions = if metadata.permissions().readonly() {
        "r--r--r--"
    } else {
        "rw-rw-rw-"
    };
    format!("{kind}{permissions}")
}

/// Emacs `file-size-human-readable` with its default 1024 based units.
fn size(bytes: u64) -> String {
    const PREFIXES: [&str; 11] = ["", "k", "M", "G", "T", "P", "E", "Z", "Y", "R", "Q"];
    let mut size = bytes as f64;
    let mut prefix = 0;
    while size >= 1024.0 && prefix + 1 < PREFIXES.len() {
        size /= 1024.0;
        prefix += 1;
    }
    let fraction = size.fract();
    if size < 10.0 && (0.05..0.95).contains(&fraction) {
        format!("{size:.1}{}", PREFIXES[prefix])
    } else {
        format!("{size:.0}{}", PREFIXES[prefix])
    }
}

/// `marginalia--time`: a relative age under two weeks, otherwise a `C` locale
/// absolute timestamp that includes the year only for earlier years.
fn time(modified: Option<SystemTime>, now: SystemTime) -> String {
    let Some(modified) = modified else {
        return String::new();
    };
    let age = now.duration_since(modified).unwrap_or_default();
    if age < MAX_RELATIVE_AGE {
        format!("{} ago", relative(age.as_secs_f64()))
    } else {
        absolute(modified, now)
    }
}

#[derive(Clone, Copy)]
struct LocalTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

#[cfg(unix)]
fn local(time: SystemTime) -> LocalTime {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let epoch = seconds.min(libc::time_t::MAX as u64) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&epoch, &mut tm) }.is_null() {
        return LocalTime {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
        };
    }
    LocalTime {
        year: tm.tm_year + 1900,
        month: tm.tm_mon.clamp(0, 11) as u32 + 1,
        day: tm.tm_mday.clamp(0, 31) as u32,
        hour: tm.tm_hour.clamp(0, 23) as u32,
        minute: tm.tm_min.clamp(0, 59) as u32,
    }
}

#[cfg(not(unix))]
fn local(time: SystemTime) -> LocalTime {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    civil_from_days((seconds / 86400) as i64, seconds % 86400)
}

fn absolute(modified: SystemTime, now: SystemTime) -> String {
    let file = local(modified);
    let current = local(now);
    let month = MONTHS[(file.month.max(1) - 1) as usize];
    if current.year > file.year {
        format!(" {:04} {month} {:02}", file.year, file.day)
    } else {
        format!(
            "{month} {:02} {:02}:{:02}",
            file.day, file.hour, file.minute
        )
    }
}

#[cfg(not(unix))]
fn civil_from_days(days: i64, seconds: u64) -> LocalTime {
    // Howard Hinnant's civil_from_days, for platforms without local time.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe as i64 + era * 400 + i64::from(month <= 2)) as i32;
    LocalTime {
        year,
        month,
        day,
        hour: (seconds / 3600 % 24) as u32,
        minute: (seconds / 60 % 60) as u32,
    }
}

/// `seconds-to-string SECS 'expanded 'abbrev` from `compat-31`.
fn relative(seconds: f64) -> String {
    const UNITS: [(&str, f64); 7] = [
        ("Y", 31_556_952.0),
        ("M", 2_629_746.0),
        ("w", 604_800.0),
        ("d", 86_400.0),
        ("h", 3_600.0),
        ("m", 60.0),
        ("s", 1.0),
    ];
    if seconds.round_ties_even() == 0.0 {
        return "0s".to_owned();
    }
    let mut unit = 0;
    while unit + 1 < UNITS.len() && seconds / UNITS[unit].1 < 1.0 {
        unit += 1;
    }
    let whole = (seconds / UNITS[unit].1).floor();
    let rest = if unit + 1 < UNITS.len() {
        ((seconds - whole * UNITS[unit].1) / UNITS[unit + 1].1).round_ties_even()
    } else {
        0.0
    };
    if rest > 0.0 {
        format!("{whole:.0}{} {rest:.0}{}", UNITS[unit].0, UNITS[unit + 1].0)
    } else {
        let count = (seconds / UNITS[unit].1).round_ties_even();
        format!("{count:.0}{}", UNITS[unit].0)
    }
}

#[cfg(unix)]
fn owner(metadata: &fs::Metadata, names: &mut OwnerNames) -> Option<String> {
    use std::os::unix::fs::MetadataExt;

    owner_text(metadata.uid(), metadata.gid(), names)
}

/// `marginalia--file-owner`: only a foreign user or group is named.
#[cfg(unix)]
fn owner_text(uid: u32, gid: u32, names: &mut OwnerNames) -> Option<String> {
    if uid == unsafe { libc::getuid() } && gid == unsafe { libc::getgid() } {
        return None;
    }
    let user = names.user(uid).to_owned();
    let group = names.group(gid).to_owned();
    Some(format!("{user}:{group}"))
}

#[cfg(not(unix))]
fn owner(_metadata: &fs::Metadata, _names: &mut OwnerNames) -> Option<String> {
    None
}

#[cfg(unix)]
fn lookup(
    mut buffer: Vec<u8>,
    call: impl Fn(&mut [u8]) -> (i32, Option<String>),
) -> Option<String> {
    // NSS entries can exceed a small buffer, so retry on ERANGE up to 64 KiB.
    loop {
        let (status, value) = call(&mut buffer);
        if status == 0 {
            return value;
        }
        if status != libc::ERANGE || buffer.len() >= 65_536 {
            return None;
        }
        buffer.resize(buffer.len() * 4, 0);
    }
}

#[cfg(unix)]
fn user_name(uid: u32) -> Option<String> {
    lookup(vec![0u8; 1024], |buffer| {
        let mut record: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                &mut record,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        let name = (status == 0 && !result.is_null()).then(|| {
            unsafe { std::ffi::CStr::from_ptr(record.pw_name) }
                .to_string_lossy()
                .into_owned()
        });
        (status, name)
    })
}

#[cfg(unix)]
fn group_name(gid: u32) -> Option<String> {
    lookup(vec![0u8; 1024], |buffer| {
        let mut record: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        let status = unsafe {
            libc::getgrgid_r(
                gid,
                &mut record,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        let name = (status == 0 && !result.is_null()).then(|| {
            unsafe { std::ffi::CStr::from_ptr(record.gr_name) }
                .to_string_lossy()
                .into_owned()
        });
        (status, name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            ".pi/tmp/annotation-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn sizes_match_emacs_file_size_human_readable() {
        for (bytes, expected) in [
            (0, "0"),
            (1, "1"),
            (999, "999"),
            (1023, "1023"),
            (1024, "1k"),
            (1536, "1.5k"),
            (4096, "4k"),
            (10240, "10k"),
            (1_048_576, "1M"),
            (1_234_567, "1.2M"),
            (5_000_000, "4.8M"),
            (1_073_741_824, "1G"),
        ] {
            assert_eq!(size(bytes), expected, "{bytes}");
        }
    }

    #[test]
    fn relative_ages_match_seconds_to_string_expanded_abbrev() {
        for (seconds, expected) in [
            (0.0, "0s"),
            (0.4, "0s"),
            (0.5, "0s"),
            (0.6, "1s"),
            (1.0, "1s"),
            (1.5, "2s"),
            (30.0, "30s"),
            (59.0, "59s"),
            (60.0, "1m"),
            (90.0, "1m 30s"),
            (3599.0, "59m 59s"),
            (3600.0, "1h"),
            (5400.0, "1h 30m"),
            (7200.0, "2h"),
            (86399.0, "23h 60m"),
            (86400.0, "1d"),
            (90000.0, "1d 1h"),
            (146_880.0, "1d 17h"),
            (604_800.0, "1w"),
            (1_209_599.0, "1w 7d"),
        ] {
            assert_eq!(relative(seconds), expected, "{seconds}");
        }
    }

    #[test]
    fn relative_age_stops_at_two_weeks() {
        let now = at(1_700_000_000);
        assert_eq!(time(Some(now), now), "0s ago");
        let under_two_weeks = now - Duration::from_secs(13 * 86_400 + 3600);
        assert_eq!(time(Some(under_two_weeks), now), "1w 6d ago");
        let two_weeks = now - MAX_RELATIVE_AGE;
        let text = time(Some(two_weeks), now);
        assert_eq!(text.len(), 12, "{text}");
        assert!(!text.ends_with("ago"), "{text}");
    }

    #[test]
    fn absolute_times_keep_the_c_locale_layout() {
        let now = at(1_775_865_600); // 2026-04-11 UTC
        let same_year = now - Duration::from_secs(15 * 86_400);
        let text = time(Some(same_year), now);
        assert_eq!(text.len(), 12, "{text}");
        assert!(MONTHS.contains(&&text[..3]), "{text}");
        assert_eq!(&text[3..4], " ", "{text}");
        assert_eq!(&text[6..7], " ", "{text}");
        assert_eq!(&text[9..10], ":", "{text}");

        let earlier_year = at(1_583_390_880); // 2020-03-05 UTC
        let text = time(Some(earlier_year), now);
        assert_eq!(text.len(), 12, "{text}");
        assert_eq!(&text[..1], " ", "{text}");
        assert_eq!(&text[1..5], "2020", "{text}");
        assert!(MONTHS.contains(&&text[6..9]), "{text}");
    }

    #[cfg(unix)]
    fn set_mtime(path: &std::path::Path, time: SystemTime) {
        use std::os::unix::ffi::OsStrExt;

        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let seconds = time.duration_since(UNIX_EPOCH).unwrap().as_secs() as libc::time_t;
        let times = [libc::timespec {
            tv_sec: seconds,
            tv_nsec: 0,
        }; 2];
        assert_eq!(
            unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) },
            0
        );
    }

    /// Field layout matches `marginalia-annotate-file` on the same directory.
    #[cfg(unix)]
    #[test]
    fn annotations_match_marginalia_field_layout() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let dir = temp_dir("layout");
        let now = SystemTime::now();
        let script = dir.join("a.txt");
        fs::write(&script, b"hello").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o4755)).unwrap();
        set_mtime(&script, now - Duration::from_secs(2 * 86_400));
        assert_eq!(
            file(
                &fs::symlink_metadata(&script).unwrap(),
                now,
                &mut OwnerNames::default()
            ),
            "-rwsr-xr-x        5        2d ago  "
        );

        let link = dir.join("good-link");
        symlink(&script, &link).unwrap();
        let text = file(
            &fs::symlink_metadata(&link).unwrap(),
            now,
            &mut OwnerNames::default(),
        );
        assert!(text.starts_with("lrwxrwxrwx"), "{text}");
        assert!(text.ends_with("0s ago  "), "{text}");

        let middling = dir.join("mid.bin");
        fs::write(&middling, vec![0u8; 12345]).unwrap();
        set_mtime(&middling, now - Duration::from_secs(20 * 86_400));
        let text = file(
            &fs::symlink_metadata(&middling).unwrap(),
            now,
            &mut OwnerNames::default(),
        );
        assert!(text.starts_with("-rw-r--r--"), "{text}");
        assert!(text.contains("12k"), "{text}");
        assert_eq!(text.len(), 10 + 2 + 7 + 2 + 12 + 2, "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn modes_describe_type_and_special_permission_bits() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let dir = temp_dir("modes");
        let modes = |path: &std::path::Path| modes(&fs::symlink_metadata(path).unwrap());
        let file = dir.join("file");
        fs::write(&file, b"").unwrap();
        for (mode, expected) in [
            (0o644, "-rw-r--r--"),
            (0o755, "-rwxr-xr-x"),
            (0o4755, "-rwsr-xr-x"),
            (0o4644, "-rwSr--r--"),
            (0o2755, "-rwxr-sr-x"),
            (0o2640, "-rw-r-S---"),
        ] {
            fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(modes(&file), expected, "{mode:o}");
        }
        assert_eq!(modes(&dir), "drwxr-xr-x");
        let sticky = dir.join("sticky");
        fs::create_dir(&sticky).unwrap();
        fs::set_permissions(&sticky, fs::Permissions::from_mode(0o1777)).unwrap();
        assert_eq!(modes(&sticky), "drwxrwxrwt");
        fs::set_permissions(&sticky, fs::Permissions::from_mode(0o1666)).unwrap();
        assert_eq!(modes(&sticky), "drw-rw-rwT");
        let link = dir.join("link");
        symlink(&file, &link).unwrap();
        assert_eq!(modes(&link), "lrwxrwxrwx");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn owner_is_only_shown_for_a_foreign_user_or_group() {
        let dir = temp_dir("owner");
        let path = dir.join("a.txt");
        fs::write(&path, b"").unwrap();
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert_eq!(owner(&metadata, &mut OwnerNames::default()), None);
        assert_eq!(user_name(u32::MAX), None);
        assert_eq!(group_name(u32::MAX), None);
        assert!(user_name(0).is_some());
        let mut names = OwnerNames::default();
        let foreign = u32::MAX - 1;
        assert_eq!(
            owner_text(foreign, foreign, &mut names),
            Some(format!("{foreign}:{foreign}")),
            "unknown ids fall back to their numbers"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
