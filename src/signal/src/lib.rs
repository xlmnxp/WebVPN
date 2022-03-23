#![warn(rust_2018_idioms)]
#![allow(dead_code)]

use std::io::{Write, Read};
use flate2::{write, read, Compression};
use anyhow::Result;

/// must_read_stdin blocks until input is received from stdin
pub fn must_read_stdin() -> Result<String> {
    let mut line = String::new();

    std::io::stdin().read_line(&mut line)?;
    line = line.trim().to_owned();
    println!();

    Ok(line)
}

/// encode encodes the input in ascii85/base85
/// It can gzip the input before encoding
pub fn encode(b: &str) -> String {
    //if COMPRESS {
    //    b = zip(b);
    //}
    println!("encode {}", b);
    let mut encoder = write::GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&smaz::compress(b.as_bytes())).unwrap();
    ascii85::encode(&encoder.finish().unwrap())
}

/// decode decodes the input from ascii85/base85
/// It ungzip the input after decoding
pub fn decode(s: &str) -> Result<String> {
    let decoded_ascii85_string = ascii85::decode(s).unwrap();
    let mut gz_decoded = read::GzDecoder::new(&decoded_ascii85_string[..]);
    let mut decoded_string = Vec::new();
    println!("decode {}", s);
    gz_decoded.read_to_end(&mut decoded_string)?;
    Ok(String::from_utf8(smaz::decompress(&decoded_string)?)?)
}