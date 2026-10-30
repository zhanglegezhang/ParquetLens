//! Bounded compact-Thrift preflight: detect plaintext encrypted footer field 8
//! before arrow-rs (built without encryption support) discards crypto fields.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Cursor<'_> {
    fn byte(&mut self) -> Result<u8, &'static str> {
        let b = *self
            .bytes
            .get(self.pos)
            .ok_or("Truncated compact metadata")?;
        self.pos += 1;
        Ok(b)
    }
    fn varint(&mut self) -> Result<u64, &'static str> {
        let mut n = 0u64;
        for shift in (0..70).step_by(7) {
            let b = self.byte()?;
            if shift == 63 && b > 1 {
                return Err("Invalid metadata varint");
            }
            n |= ((b & 127) as u64) << shift;
            if b < 128 {
                return Ok(n);
            }
        }
        Err("Invalid metadata varint")
    }
    fn advance(&mut self, n: u64) -> Result<(), &'static str> {
        let n = usize::try_from(n).map_err(|_| "Metadata length overflow")?;
        self.pos = self
            .pos
            .checked_add(n)
            .filter(|p| *p <= self.bytes.len())
            .ok_or("Truncated metadata value")?;
        Ok(())
    }
    fn object(&mut self, depth: u8, root: bool) -> Result<bool, &'static str> {
        if depth > 64 {
            return Err("Metadata nesting exceeds safety limit");
        }
        let mut field = 0i64;
        loop {
            let header = self.byte()?;
            if header == 0 {
                return Ok(false);
            };
            let delta = header >> 4;
            field = if delta > 0 {
                field
                    .checked_add(delta as i64)
                    .ok_or("Metadata field id overflow")?
            } else {
                let n = self.varint()?;
                ((n >> 1) as i64) ^ -((n & 1) as i64)
            };
            if !(1..=i16::MAX as i64).contains(&field) {
                return Err("Invalid metadata field id");
            }
            if root && field == 8 && header & 15 == 12 {
                return Ok(true);
            }
            self.skip(header & 15, depth + 1, true)?;
        }
    }
    fn skip(&mut self, t: u8, depth: u8, field: bool) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("Metadata nesting exceeds safety limit");
        }
        match t {
            1 | 2 => {
                if !field {
                    self.advance(1)?;
                }
            }
            3 => self.advance(1)?,
            4..=6 => {
                self.varint()?;
            }
            7 => self.advance(8)?,
            8 => {
                let n = self.varint()?;
                self.advance(n)?;
            }
            9 | 10 => {
                let h = self.byte()?;
                let n = if h >> 4 == 15 {
                    self.varint()?
                } else {
                    (h >> 4) as u64
                };
                if n > self.bytes.len() as u64 {
                    return Err("Invalid metadata collection length");
                }
                for _ in 0..n {
                    self.skip(h & 15, depth + 1, false)?;
                }
            }
            11 => {
                let n = self.varint()?;
                if n > self.bytes.len() as u64 {
                    return Err("Invalid map length");
                }
                if n > 0 {
                    let h = self.byte()?;
                    for _ in 0..n {
                        self.skip(h >> 4, depth + 1, false)?;
                        self.skip(h & 15, depth + 1, false)?;
                    }
                }
            }
            12 => {
                self.object(depth + 1, false)?;
            }
            _ => return Err("Unknown compact metadata type"),
        }
        Ok(())
    }
}
pub fn encrypted(bytes: &[u8]) -> Result<bool, &'static str> {
    Cursor { bytes, pos: 0 }.object(0, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_preflight_is_bounded() {
        assert!(encrypted(&[]).is_err());
        assert!(
            encrypted(&[
                0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01
            ])
            .is_err()
        );
        let mut seed = 1u64;
        for length in 0..256 {
            let mut bytes = vec![0; length];
            for b in &mut bytes {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                *b = (seed >> 32) as u8;
            }
            let _ = encrypted(&bytes);
        }
    }
}
