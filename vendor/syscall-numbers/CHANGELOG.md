# Change log

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [4.0.2] - 2025-02-21

### Changed

- Migrated from 2021 edition to 2024.

## [4.0.1] - 2024-12-20

### Changed

- Updated `crates.io` documentation.

## [4.0.0] - 2024-07-28

### Added

- Added support for the following platforms:
  - LoongArch 64-bit (`loongarch64`).
  - RISC-V 32-bit (`riscv32`).

- Added the following system calls to multiple platforms (when implemented):
  - `memfd_secret`
  - `process_mrelease`
  - `futex_waitv`
  - `set_mempolicy_home_node`
  - `cachestat`
  - `fchmodat2`

### Changed

- The condition for the definition of the `syscall_numbers::native` module changed for
  the following platforms:
  - AMD64 (`x86_64`).
  - X86_32 (`x32`).
  - MIPS N32 (`mipsn32`).

  > ⚠️ **This is a breaking change**.

### Removed

- The `syscall_numbers::native` module is now **undefined** for the following platforms:
  - MicroBlaze (`microblaze`).
  - OpenRISC 1000 (`or1k`).
  - SuperH (`sh`).

  > ⚠️ **This is a breaking change**.

## [3.1.1] - 2024-03-27

### Changed

- Moved repository to `codeberg.org`.

## [3.1.0] - 2023-04-12

### Changed

- Added support for `no_std`.

  Thank you, [*BrainStackOverFlow*](https://github.com/BrainStackOverFlow).

## [3.0.0] - 2022-05-08

### Changed

- The `riscv64::SYS_fstatat` was renamed to `riscv64::SYS_newfstatat`, to correct the system call
  name on RISC-V 64-bits.
  > ⚠️ **This is a breaking change**.

### Added

- Use hexadecimal system call numbers.
- Added the following system calls: `process_madvise`, `epoll_pwait2`, `mount_setattr`,
  `landlock_create_ruleset`, `landlock_add_rule`, `landlock_restrict_self`.

## [2.0.0] - 2021-10-25

### Changed

- Migrated Rust edition to 2021.
  > ⚠️ **This is a breaking change**.

## [1.0.0] - 2021-08-12

### Added

- Initial release.
