# XArray 

`XArray` is an abstract data type functioning like an expansive array of items where each item must be an 8-byte object, such as `Arc<T>` or `Box<T>`.
User-stored pointers must have a minimum alignment of 4 bytes. `XArray` facilitates efficient sequential access to adjacent entries,
supporting multiple concurrent reads and exclusively allowing one write operation at a time.

## Features

- **Cursors:** Provide cursors for precise and efficient iteration over the array. Cursors have both immutable and mutable versions. One can hold multiple immutable cursors or hold a mutable cursor exclusively at a time. 
- **Marking:** Provide ability to mark entries and the XArray itself for easy state tracking.
- **Generics:** Generic implementation that can work with any entry type that fits the use case.
- **Copy-on-Write (COW):** Efficient cloning of XArrays with shared structure until mutation.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
xarray = "0.1.0"
```

## Usage
This crate is developed in `no_std` environment, but std users can still use this crate with `--feature="std"`:

The following section covers how to interact with `XArray` including creating an `XArray`, using cursors, marking, cloning, and more.

---
### Creating an `XArray`:
```rust
// In std environment
extern crate alloc;

use alloc::sync::Arc;
use xarray::XArray;

// Create a new XArray instance
let mut xarray: XArray<Arc<i32>> = XArray::new();
```

- Users should declare the type of items (Arc<i32>) stored in the XArray, and the item type should implement `ItemEntry` trait. 
- We implement `ItemEntry` for `alloc::sync::Arc` and `alloc::sync::Box` by default, hence std users can use them directly.



### Using Cursor
```rust
extern crate alloc;

use alloc::sync::Arc;
use xarray::XArray;

let mut xarray_arc: XArray<Arc<i32>> = XArray::new();

let mut cursor = xarray_arc.cursor_mut(0);
// Store the Arc at the index range 0~10000.
for i in 0..10000 {
    let value = Arc::new(i * 2);
    cursor.store(value);
    cursor.next();
}

cursor.reset_to(0);
for i in 0..10000 {
    let value = cursor.load().unwrap();
    assert!(*value.as_ref() == i * 2);
    cursor.next();
}
```

### Using Marks

Here is an example of using marks for the stored pages in the XArray, where PageMark represents the states of each individual Page:
```rust
extern crate alloc;

use alloc::sync::Arc;
use xarray::{XArray, XMark, StdMutex};

#[derive(Clone, Copy)]

enum PageMark {
    DirtyPage 
    ...
}

impl From<PageState> for XMark {
    fn from(mark: PageState) -> Self {
        match mark {
            PageState::DirtyPage => Self::Mark0,
            ...
        }
    }
}

let mut pages: XArray<Page, StdMutex, PageState> = XArray::new();

let mut cursor = pages.cursor_mut(1000);
cursor.store(Page::alloc_zero());
// Mark the Page as DirtyPage.
cursor.set_mark(PageState::DirtyPage).unwrap();
assert!(cursor.is_marked(PageState::DirtyPage));
```
- Items and the `XArray` can have up to three distinct marks by default, with each mark independently maintained.
- Users need to use a struct to represent the marks that need to be used. For the situation where multiple marks are required, these marks are typically encapsulated within an enumeration class.
- If users want to use a struct `M` for marks, they should implement `From<M>` trait for `XMark` and declare `M` in the generics list of XArray.

### Copy-On-Write (COW) Clone
```rust
use std::sync::Arc;
use xarray::{XArray};

let mut xarray: XArray<Arc<i32>> = XArray::new();

// Store values
let value = Arc::new(10);
xarray.store(1, value.clone());
assert_eq!(*xarray.load(1).unwrap().as_ref(), 10);

// Clone the XArray
let mut xarray_clone = xarray.clone();
assert_eq!(*xarray_clone.load(1).unwrap().as_ref(), 10);

// Store a new value in the clone
let new_value = Arc::new(100);
xarray_clone.store(1, new_value);

// The original XArray is unaffected by changes in the clone
assert_eq!(*xarray.load(1).unwrap().as_ref(), 10);
assert_eq!(*xarray_clone.load(1).unwrap().as_ref(), 100);
```

### Iteration
```rust
use std::sync::Arc;
use xarray::XArray;

let mut xarray: XArray<Arc<i32>> = XArray::new();

// Store item to even index in the range 100~200.
for i in 100..200 {
    if i % 2 == 0 {
        let value = Arc::new(i * 2);
        cursor.store(value);
    }
    cursor.next();
}

// Iterate at the range 100~200.
let mut count = 0;
for item in xarray.range(100..200) {
    count += 1;
}
assert_eq!(count == 50);
```

## License


