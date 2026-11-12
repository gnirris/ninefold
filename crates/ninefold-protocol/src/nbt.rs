//! Just enough NBT to walk past it.
//!
//! An item stack on the wire ends with an optional NBT compound — enchantments, a custom name,
//! the lore on a written book. Nothing here needs to read any of it, but everything after it in
//! the packet does, and a window full of items is one slot after another with no lengths to skip
//! by. So this walks the tree and counts the bytes.
//!
//! The tag numbers are the ones NBT has had since Notch wrote it: 1 byte, 2 short, 3 int,
//! 4 long, 5 float, 6 double, 7 byte array, 8 string, 9 list, 10 compound, 11 int array.

use crate::{Error, Reader, Result};

/// Deeper than this and something is wrong with the stream, not with the data.
const MAX_DEPTH: u32 = 64;

/// Skip the optional NBT blob at the cursor: a tag byte, and the value if it is not `TAG_End`.
///
/// This is the shape item stacks use — a single `0` where there is no extra data.
pub fn skip_optional(r: &mut Reader) -> Result<()> {
    let tag = r.u8()?;
    if tag == 0 {
        return Ok(());
    }
    // A named root: the name comes before the payload.
    skip_name(r)?;
    skip_payload(r, tag, 0)
}

fn skip_name(r: &mut Reader) -> Result<()> {
    let len = usize::from(r.u16()?);
    for _ in 0..len {
        r.u8()?;
    }
    Ok(())
}

/// Skip one value of the given tag type.
pub fn skip_payload(r: &mut Reader, tag: u8, depth: u32) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::BadLength);
    }
    match tag {
        0 => {}
        1 => {
            r.u8()?;
        }
        2 => {
            r.u16()?;
        }
        3 | 5 => {
            r.i32()?;
        }
        4 | 6 => {
            r.i64()?;
        }
        7 => {
            let len = r.i32()?;
            take(r, len, 1)?;
        }
        8 => skip_name(r)?,
        9 => {
            let inner = r.u8()?;
            let len = r.i32()?;
            if len < 0 {
                return Err(Error::BadLength);
            }
            for _ in 0..len {
                skip_payload(r, inner, depth + 1)?;
            }
        }
        10 => loop {
            let inner = r.u8()?;
            if inner == 0 {
                break;
            }
            skip_name(r)?;
            skip_payload(r, inner, depth + 1)?;
        },
        11 => {
            let len = r.i32()?;
            take(r, len, 4)?;
        }
        _ => return Err(Error::BadLength),
    }
    Ok(())
}

/// Step over `count * each` bytes, refusing a length that was never sent.
fn take(r: &mut Reader, count: i32, each: usize) -> Result<()> {
    let count = usize::try_from(count).map_err(|_| Error::BadLength)?;
    let bytes = count.checked_mul(each).ok_or(Error::BadLength)?;
    for _ in 0..bytes {
        r.u8()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_data_is_one_byte() {
        let mut r = Reader::new(&[0, 42]);
        skip_optional(&mut r).unwrap();
        assert_eq!(r.u8().unwrap(), 42, "stopped right after the zero");
    }

    #[test]
    fn a_compound_is_walked_to_its_end() {
        // { } named "tag", then a sentinel byte.
        let blob = [
            10, // compound
            0, 3, b't', b'a', b'g', // name
            0,  // TAG_End closing the compound
            42, // sentinel
        ];
        let mut r = Reader::new(&blob);
        skip_optional(&mut r).unwrap();
        assert_eq!(r.u8().unwrap(), 42);
    }

    #[test]
    fn nested_lists_and_strings_are_counted() {
        // { "s": "hi", "l": [ 1b, 2b ] }
        let mut blob = vec![10, 0, 0];
        blob.extend([8, 0, 1, b's', 0, 2, b'h', b'i']);
        blob.extend([9, 0, 1, b'l', 1, 0, 0, 0, 2, 7, 9]);
        blob.push(0);
        blob.push(42);
        let mut r = Reader::new(&blob);
        skip_optional(&mut r).unwrap();
        assert_eq!(r.u8().unwrap(), 42);
    }

    #[test]
    fn a_truncated_tree_is_an_error_rather_than_a_panic() {
        let blob = [10, 0, 0, 8, 0, 1, b's', 0, 9];
        assert!(skip_optional(&mut Reader::new(&blob)).is_err());
    }

    #[test]
    fn a_tag_number_that_does_not_exist_is_refused() {
        assert!(skip_optional(&mut Reader::new(&[99, 0, 0])).is_err());
    }
}
