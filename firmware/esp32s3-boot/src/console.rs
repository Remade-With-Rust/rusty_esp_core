//! The ROM's console, one character at a time, at whatever the ROM set
//! (115200 on UART0 unless the eFuse says otherwise). Every line starts
//! with the loader's name so a monitor can tell its lines from the app's.

use core::fmt;

pub struct Console;

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            // SAFETY: a ROM routine with no preconditions.
            unsafe {
                if b == b'\n' {
                    crate::rom::uart_tx_one_char(b'\r');
                }
                crate::rom::uart_tx_one_char(b);
            }
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = write!($crate::console::Console, "rusty_esp_boot: ");
        let _ = writeln!($crate::console::Console, $($arg)*);
    }};
}
