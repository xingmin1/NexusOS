# rev_buf_reader

[![GitHub Actions Workflow](https://github.com/andre-vm/rev_buf_reader/workflows/Tests/badge.svg)](https://github.com/andre-vm/rev_buf_reader/actions)
[![docs](https://docs.rs/rev_buf_reader/badge.svg)](https://docs.rs/rev_buf_reader/latest/rev_buf_reader/)
[![license](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](https://github.com/andre-vm/rev_buf_reader/)
[![Crates.io](https://img.shields.io/crates/v/rev_buf_reader.svg)](https://crates.io/crates/rev_buf_reader)


This Rust crate provides a buffered reader capable of reading chunks of bytes of a data stream in reverse order. Its implementation is an adapted copy of BufReader from the nightly std::io.

# Usage

## Reading chunks of bytes in reverse order:

```rust
extern crate rev_buf_reader;

use rev_buf_reader::RevBufReader;
use std::io::{self, Read};

let data = [0, 1, 2, 3, 4, 5, 6, 7];
let inner = io::Cursor::new(&data);
let mut reader = RevBufReader::new(inner);

let mut buffer = [0, 0, 0];
assert_eq!(reader.read(&mut buffer).ok(), Some(3));
assert_eq!(buffer, [5, 6, 7]);

let mut buffer = [0, 0, 0, 0, 0];
assert_eq!(reader.read(&mut buffer).ok(), Some(5));
assert_eq!(buffer, [0, 1, 2, 3, 4]);
```

## Reading text lines in reverse order:

```rust
extern crate rev_buf_reader;

use rev_buf_reader::RevBufReader;
use std::io::{self, BufRead};

let data = "This\nis\na sentence";
let inner = io::Cursor::new(&data);
let reader = RevBufReader::new(inner);
let mut lines = reader.lines();

assert_eq!(lines.next().unwrap().unwrap(), "a sentence".to_string());
assert_eq!(lines.next().unwrap().unwrap(), "is".to_string());
assert_eq!(lines.next().unwrap().unwrap(), "This".to_string());
assert!(lines.next().is_none());
```

# Features

**rev_buf_reader** has one feature: `read_initializer`, which corresponds to an
experimental feature of nightly Rust. If you use it in your project by adding
`#![feature(read_initializer)]`, you'll need to enable it for **rev_buf_reader**
as well in your Cargo.toml.
