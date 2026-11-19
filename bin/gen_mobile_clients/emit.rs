//! Tiny string builder for codegen.

use std::fmt::Write as _;

#[derive(Default)]
pub struct SourceBuf(String);

impl SourceBuf {
    pub fn line(&mut self, s: impl AsRef<str>) {
        let _ = writeln!(self.0, "{}", s.as_ref());
    }

    pub fn blank(&mut self) {
        self.0.push('\n');
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

macro_rules! w {
    ($buf:expr, $($t:tt)*) => {{
        $buf.line(format!($($t)*));
    }};
}

pub(crate) use w;
