// SPDX-License-Identifier: Apache-2.0
// Extracted from Zed gpui_util; modified for semacs on 2026-10-31.
// See THIRD_PARTY.md.

#[inline]
pub const fn is_utf8_char_boundary(u8: u8) -> bool {
    // This is bit magic equivalent to: b < 128 || b >= 192
    (u8 as i8) >= -0x40
}

macro_rules! debug_panic {
    ( $($fmt_arg:tt)* ) => {
        if cfg!(debug_assertions) {
            panic!( $($fmt_arg)* );
        } else {
            let backtrace = std::backtrace::Backtrace::capture();
            log::error!("{}\n{:?}", format_args!($($fmt_arg)*), backtrace);
        }
    };
}

pub(super) use debug_panic;
