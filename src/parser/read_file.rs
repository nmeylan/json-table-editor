use std::io::{self, Read};

/// A custom reader that replaces LF (`\n`) with CRLF (`\r\n`)
pub struct LfToCrlfReader<R: Read> {
    inner: R,
    cursor: usize,
}

impl<R: Read> LfToCrlfReader<R> {
    pub(crate) fn new(inner: R) -> LfToCrlfReader<R> {
        LfToCrlfReader {
            inner,
            cursor: 0,
        }
    }
}

impl<R: Read> Read for LfToCrlfReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.cursor = 0;
        let mut tmp_buf = vec![0; 1024 * 8];
        match self.inner.read(&mut tmp_buf) {
            Ok(0) => return Ok(0), // End of file
            Ok(n) => {
                let mut i = 0;
                while i < n {
                    if tmp_buf[i] <= 0x20 { // ignore any char <= SPACE
                        buf[i] = 0;
                    }  else {
                        buf[i] = tmp_buf[i];
                    }
                    i += 1;
                }
                self.cursor = n;
            },
            Err(e) => return Err(e),
        }

        Ok(self.cursor)
    }
}