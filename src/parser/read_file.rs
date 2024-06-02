use std::io::{self, Read};

pub struct LfToCrlfReader<R: Read> {
    inner: R,
}

impl<R: Read> LfToCrlfReader<R> {
    pub(crate) fn new(inner: R) -> LfToCrlfReader<R> {
        LfToCrlfReader {
            inner,
        }
    }
}

impl<R: Read> Read for LfToCrlfReader<R> {
    fn read(&mut self, mut buf: &mut [u8]) -> io::Result<usize> {
        let mut read_bytes = 0;
        match self.inner.read(&mut buf) {
            Ok(0) => return Ok(0), // End of file
            Ok(n) => {
                // let mut i = 0;
                // while i < n {
                //     if buf[i] <= 0x20 { // ignore any char <= SPACE
                //         buf[i] = 0;
                //     }
                //     i += 1;
                // }
                read_bytes = n;
            }
            Err(e) => return Err(e),
        }

        Ok(read_bytes)
    }
}