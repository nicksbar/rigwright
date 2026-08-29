use anyhow::{bail, Result};

/// Builds semicolon-terminated CAT frames used by Yaesu and Kenwood. A small
/// set of Kenwood commands has a documented numeric suffix, such as `MA0`.
pub fn encode(command: &str, parameter: Option<&str>) -> Result<Vec<u8>> {
    let valid_command = match command.as_bytes() {
        [first, second] => first.is_ascii_alphabetic() && second.is_ascii_alphabetic(),
        [first, second, suffix] => {
            first.is_ascii_alphabetic() && second.is_ascii_alphabetic() && suffix.is_ascii_digit()
        }
        _ => false,
    };
    if !valid_command {
        bail!("CAT command must contain two ASCII letters, optionally followed by a digit");
    }
    let parameter = parameter.unwrap_or_default();
    if !parameter.is_ascii() || parameter.contains(';') {
        bail!("CAT parameter must be ASCII and cannot contain a terminator");
    }

    let mut frame = command.to_ascii_uppercase().into_bytes();
    frame.extend_from_slice(parameter.as_bytes());
    frame.push(b';');
    Ok(frame)
}

/// Splits one read buffer into complete CAT responses and an incomplete tail.
pub fn decode_frames(buffer: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut frames = Vec::new();
    let mut start = 0;
    for (index, byte) in buffer.iter().enumerate() {
        if *byte == b';' {
            frames.push(buffer[start..=index].to_vec());
            start = index + 1;
        }
    }
    (frames, buffer[start..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_canonical_cat_frame() {
        assert_eq!(encode("fa", Some("014250000")).unwrap(), b"FA014250000;");
        assert_eq!(encode("ma0", Some("001")).unwrap(), b"MA0001;");
    }

    #[test]
    fn preserves_incomplete_tail() {
        let (frames, tail) = decode_frames(b"FA014250000;MD02;IF");
        assert_eq!(frames, [b"FA014250000;".to_vec(), b"MD02;".to_vec()]);
        assert_eq!(tail, b"IF");
    }
}
