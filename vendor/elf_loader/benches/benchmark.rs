use std::{env::consts, path::PathBuf, sync::OnceLock};

use criterion::{Criterion, criterion_group, criterion_main};
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use libloading::Library;

const TARGET_DIR: Option<&'static str> = option_env!("CARGO_TARGET_DIR");
static TARGET_TRIPLE: OnceLock<String> = OnceLock::new();

fn lib_path(file_name: &str) -> String {
    let path: PathBuf = TARGET_DIR.unwrap_or("target").into();
    path.join(TARGET_TRIPLE.get().unwrap())
        .join("release")
        .join(file_name)
        .to_str()
        .unwrap()
        .to_string()
}

const PACKAGE_NAME: [&str; 3] = ["a", "b", "c"];

fn compile() {
    static ONCE: ::std::sync::Once = ::std::sync::Once::new();
    ONCE.call_once(|| {
        let arch = consts::ARCH;
        if arch.contains("x86_64") {
            TARGET_TRIPLE
                .set("x86_64-unknown-linux-gnu".to_string())
                .unwrap();
        } else if arch.contains("riscv64") {
            TARGET_TRIPLE
                .set("riscv64gc-unknown-linux-gnu".to_string())
                .unwrap();
        } else if arch.contains("aarch64") {
            TARGET_TRIPLE
                .set("aarch64-unknown-linux-gnu".to_string())
                .unwrap();
        } else {
            unimplemented!()
        }

        for name in PACKAGE_NAME {
            let mut cmd = ::std::process::Command::new("cargo");
            cmd.arg("rustc")
                .arg("-r")
                .arg("-p")
                .arg(name)
                .arg("--target")
                .arg(TARGET_TRIPLE.get().unwrap().as_str())
                .arg("--")
                .arg("-C")
                .arg("panic=abort");
            assert!(
                cmd.status()
                    .expect("could not compile the test helpers!")
                    .success()
            );
        }
    });
}

fn load_benchmark(c: &mut Criterion) {
    compile();
    let path = lib_path("liba.so");
    c.bench_function("elf_loader:new", |b| {
        b.iter(|| {
            let mut loader = Loader::<MmapImpl>::new();
            let liba = loader
                .easy_load_dylib(ElfFile::from_path(&path).unwrap())
                .unwrap();
            let _ = liba.easy_relocate([].iter(), &|_| None).unwrap();
        });
    });
    c.bench_function("libloading:new", |b| {
        b.iter(|| {
            unsafe { Library::new(&path).unwrap() };
        })
    });
}

fn get_symbol_benchmark(c: &mut Criterion) {
    compile();
    let path = lib_path("liba.so");
    let mut loader = Loader::<MmapImpl>::new();
    let liba = loader
        .easy_load_dylib(ElfFile::from_path(&path).unwrap())
        .unwrap();
    let lib1 = liba.easy_relocate([].iter(), &|_| None).unwrap();
    let lib2 = unsafe { Library::new(path).unwrap() };
    c.bench_function("elf_loader:get", |b| {
        b.iter(|| unsafe { lib1.get::<fn(i32, i32) -> i32>("a").unwrap() })
    });
    c.bench_function("libloading:get", |b| {
        b.iter(|| {
            unsafe { lib2.get::<fn(i32, i32) -> i32>("a".as_bytes()).unwrap() };
        })
    });
}

criterion_group!(benches, load_benchmark, get_symbol_benchmark);
criterion_main!(benches);
