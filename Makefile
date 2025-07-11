# SPDX-License-Identifier: MPL-2.0

# =========================== Makefile options. ===============================

# Global build options.
ARCH ?= riscv64
BENCHMARK ?= none
BOOT_METHOD ?= grub-rescue-iso
BOOT_PROTOCOL ?= multiboot2
BUILD_SYSCALL_TEST ?= 0
ENABLE_KVM ?= 0
INTEL_TDX ?= 0
MEM ?= 1G
OVMF ?= on
RELEASE ?= 0
RELEASE_LTO ?= 0
LOG_LEVEL ?= error
SCHEME ?= "oscomp-riscv"
SMP ?= 1
OSTD_TASK_STACK_SIZE_IN_PAGES ?= 64
# End of global build options.

# GDB debugging and profiling options.
GDB_TCP_PORT ?= 12345
GDB_PROFILE_FORMAT ?= flame-graph
GDB_PROFILE_COUNT ?= 200
GDB_PROFILE_INTERVAL ?= 0.1
# End of GDB options.

# The Makefile provides a way to run arbitrary tests in the kernel
# mode using the kernel command line.
# Here are the options for the auto test feature.
AUTO_TEST ?= none
EXTRA_BLOCKLISTS_DIRS ?= ""
SYSCALL_TEST_DIR ?= /tmp
FEATURES ?=
# End of auto test features.

# Network settings
# NETDEV possible values are user,tap
NETDEV ?= user
VHOST ?= off
# End of network settings

# ========================= End of Makefile options. ==========================

CARGO_OSDK := ~/.cargo/bin/cargo-osdk

CARGO_OSDK_ARGS := --target-arch=$(ARCH) --kcmd-args="ostd.log_level=$(LOG_LEVEL)"

ifeq ($(AUTO_TEST), syscall)
BUILD_SYSCALL_TEST := 1
CARGO_OSDK_ARGS += --kcmd-args="SYSCALL_TEST_DIR=$(SYSCALL_TEST_DIR)"
CARGO_OSDK_ARGS += --kcmd-args="EXTRA_BLOCKLISTS_DIRS=$(EXTRA_BLOCKLISTS_DIRS)"
CARGO_OSDK_ARGS += --init-args="/opt/syscall_test/run_syscall_test.sh"
else ifeq ($(AUTO_TEST), test)
	ifneq ($(SMP), 1)
		CARGO_OSDK_ARGS += --kcmd-args="BLOCK_UNSUPPORTED_SMP_TESTS=1"
	endif
CARGO_OSDK_ARGS += --init-args="/test/run_general_test.sh"
else ifeq ($(AUTO_TEST), boot)
CARGO_OSDK_ARGS += --init-args="/test/boot_hello.sh"
else ifeq ($(AUTO_TEST), vsock)
export VSOCK=on
CARGO_OSDK_ARGS += --init-args="/test/run_vsock_test.sh"
endif

ifeq ($(RELEASE_LTO), 1)
CARGO_OSDK_ARGS += --profile release-lto
OSTD_TASK_STACK_SIZE_IN_PAGES = 8
else ifeq ($(RELEASE), 1)
CARGO_OSDK_ARGS += --release
OSTD_TASK_STACK_SIZE_IN_PAGES = 8
endif

# If the BENCHMARK is set, we will run the benchmark in the kernel mode.
ifneq ($(BENCHMARK), none)
CARGO_OSDK_ARGS += --init-args="/benchmark/common/bench_runner.sh $(BENCHMARK) asterinas"
endif

ifeq ($(INTEL_TDX), 1)
BOOT_METHOD = grub-qcow2
BOOT_PROTOCOL = linux-efi-handover64
CARGO_OSDK_ARGS += --scheme tdx
endif

ifneq ($(SCHEME), "")
CARGO_OSDK_ARGS += --scheme $(SCHEME)
else
CARGO_OSDK_ARGS += --boot-method="$(BOOT_METHOD)"
endif

ifdef FEATURES
CARGO_OSDK_ARGS += --features="$(FEATURES)"
endif

# To test the linux-efi-handover64 boot protocol, we need to use Debian's
# GRUB release, which is installed in /usr/bin in our Docker image.
ifeq ($(BOOT_PROTOCOL), linux-efi-handover64)
CARGO_OSDK_ARGS += --grub-mkrescue=/usr/bin/grub-mkrescue
CARGO_OSDK_ARGS += --grub-boot-protocol="linux"
# FIXME: GZIP self-decompression (--encoding gzip) triggers CPU faults
CARGO_OSDK_ARGS += --encoding raw
else ifeq ($(BOOT_PROTOCOL), linux-legacy32)
CARGO_OSDK_ARGS += --linux-x86-legacy-boot
CARGO_OSDK_ARGS += --grub-boot-protocol="linux"
else
CARGO_OSDK_ARGS += --grub-boot-protocol=$(BOOT_PROTOCOL)
endif

ifeq ($(ENABLE_KVM), 1)
CARGO_OSDK_ARGS += --qemu-args="-accel kvm"
endif

# Skip GZIP to make encoding and decoding of initramfs faster
ifeq ($(INITRAMFS_SKIP_GZIP),1)
CARGO_OSDK_INITRAMFS_OPTION := --initramfs=$(realpath test/build/initramfs.cpio)
CARGO_OSDK_ARGS += $(CARGO_OSDK_INITRAMFS_OPTION)
endif

# Pass make variables to all subdirectory makes
export

# Basically, non-OSDK crates do not depend on Aster Frame and can be checked
# or tested without OSDK.
NON_OSDK_CRATES := \
	ostd/libs/align_ext \
	ostd/libs/id-alloc \
	ostd/libs/linux-bzimage/builder \
	ostd/libs/linux-bzimage/boot-params \
	ostd/libs/ostd-macros \
	ostd/libs/ostd-test \
	ostd/libs/int-to-c-enum \
	ostd/libs/int-to-c-enum/derive

# In contrast, OSDK crates depend on OSTD (or being `ostd` itself)
# and need to be built or tested with OSDK.
OSDK_CRATES := \
	osdk/test-kernel \
	ostd \
	ostd/libs/linux-bzimage/setup \
	kernel

# OSDK dependencies
OSDK_SRC_FILES := \
	$(shell find osdk/Cargo.toml osdk/Cargo.lock osdk/src -type f)

.PHONY: all
all: kernel-rv

# Install or update OSDK from source
# To uninstall, do `cargo uninstall cargo-osdk`
.PHONY: install_osdk
install_osdk:
	@# The `OSDK_LOCAL_DEV` environment variable is used for local development
	@# without the need to publish the changes of OSDK's self-hosted
	@# dependencies to `crates.io`.
	@OSDK_LOCAL_DEV=1 cargo install cargo-osdk --path osdk

# This will install and update OSDK automatically
$(CARGO_OSDK): $(OSDK_SRC_FILES)
	@$(MAKE) --no-print-directory install_osdk

.PHONY: check_osdk
check_osdk:
	@cd osdk && cargo clippy -- -D warnings

.PHONY: test_osdk
test_osdk:
	@cd osdk && \
		OSDK_LOCAL_DEV=1 cargo build && \
		OSDK_LOCAL_DEV=1 cargo test

.PHONY: setup_cargo_config
setup_cargo_config:
	@mkdir -p .cargo
	@cp cargo/config.toml .cargo/config.toml

.PHONY: initramfs
initramfs:
	@$(MAKE) --no-print-directory -C test

# .PHONY: prepare_osdk_bundle
# prepare_osdk_bundle:
# 	@mkdir -p target/osdk/aster-nix-run-base
# 	@cp Cargo.lock target/osdk/aster-nix-run-base/Cargo.lock

.PHONY: build
build: setup_cargo_config $(CARGO_OSDK)
	@cd kernel && cargo osdk build $(CARGO_OSDK_ARGS)

.PHONY: tools
tools:
	@cd kernel/libs/comp-sys && cargo install --path cargo-component

.PHONY: run
run: $(CARGO_OSDK)
	@cd kernel && cargo osdk run $(CARGO_OSDK_ARGS)
# Check the running status of auto tests from the QEMU log
ifeq ($(AUTO_TEST), syscall)
	@tail --lines 100 qemu.log | grep -q "^.* of .* test cases passed." \
		|| (echo "Syscall test failed" && exit 1)
else ifeq ($(AUTO_TEST), test)
	@tail --lines 100 qemu.log | grep -q "^All general tests passed." \
		|| (echo "General test failed" && exit 1)
else ifeq ($(AUTO_TEST), boot)
	@tail --lines 100 qemu.log | grep -q "^Successfully booted." \
		|| (echo "Boot test failed" && exit 1)
else ifeq ($(AUTO_TEST), vsock)
	@tail --lines 100 qemu.log | grep -q "^Vsock test passed." \
		|| (echo "Vsock test failed" && exit 1)
endif

.PHONY: gdb_server
gdb_server: $(CARGO_OSDK)
	@cd kernel && cargo osdk run $(CARGO_OSDK_ARGS) --gdb-server wait-client,vscode,addr=:$(GDB_TCP_PORT)

.PHONY: gdb_client
gdb_client: $(CARGO_OSDK)
	@cd kernel && cargo osdk debug $(CARGO_OSDK_ARGS) --remote :$(GDB_TCP_PORT)

.PHONY: profile_server
profile_server: initramfs $(CARGO_OSDK)
	@cd kernel && cargo osdk run $(CARGO_OSDK_ARGS) --gdb-server addr=:$(GDB_TCP_PORT)

.PHONY: profile_client
profile_client: initramfs $(CARGO_OSDK)
	@cd kernel && cargo osdk profile $(CARGO_OSDK_ARGS) --remote :$(GDB_TCP_PORT) \
		--samples $(GDB_PROFILE_COUNT) --interval $(GDB_PROFILE_INTERVAL) --format $(GDB_PROFILE_FORMAT)

.PHONY: test
test:
	@for dir in $(NON_OSDK_CRATES); do \
		(cd $$dir && cargo test) || exit 1; \
	done

.PHONY: ktest
ktest: initramfs $(CARGO_OSDK)
	@# Exclude linux-bzimage-setup from ktest since it's hard to be unit tested
	@for dir in $(OSDK_CRATES); do \
		[ $$dir = "ostd/libs/linux-bzimage/setup" ] && continue; \
		echo "[make] Testing $$dir"; \
		(cd $$dir && OVMF=off cargo osdk test $(CARGO_OSDK_INITRAMFS_OPTION)) || exit 1; \
		tail --lines 10 qemu.log | grep -q "^\\[ktest runner\\] All crates tested." \
			|| (echo "Test failed" && exit 1); \
	done

.PHONY: docs
docs: $(CARGO_OSDK)
	@for dir in $(NON_OSDK_CRATES); do \
		(cd $$dir && cargo doc --no-deps) || exit 1; \
	done
	@for dir in $(OSDK_CRATES); do \
		(cd $$dir && cargo osdk doc --no-deps) || exit 1; \
	done
	@echo "" 						# Add a blank line
	@cd docs && mdbook build 				# Build mdBook

.PHONY: format
format:
	@./tools/format_all.sh
	@$(MAKE) --no-print-directory -C test format

.PHONY: check
check: initramfs $(CARGO_OSDK)
	@./tools/format_all.sh --check   	# Check Rust format issues
	@# Check if STD_CRATES and NOSTD_CRATES combined is the same as all workspace members
	@sed -n '/^\[workspace\]/,/^\[.*\]/{/members = \[/,/\]/p}' Cargo.toml | \
		grep -v "members = \[" | tr -d '", \]' | \
		sort > /tmp/all_crates
	@echo $(NON_OSDK_CRATES) $(OSDK_CRATES) | tr ' ' '\n' | sort > /tmp/combined_crates
	@diff -B /tmp/all_crates /tmp/combined_crates || \
		(echo "Error: The combination of STD_CRATES and NOSTD_CRATES" \
			"is not the same as all workspace members" && exit 1)
	@rm /tmp/all_crates /tmp/combined_crates
	@for dir in $(NON_OSDK_CRATES); do \
		echo "Checking $$dir"; \
		(cd $$dir && cargo clippy -- -D warnings) || exit 1; \
	done
	@for dir in $(OSDK_CRATES); do \
		echo "Checking $$dir"; \
		(cd $$dir && cargo osdk clippy -- -- -D warnings) || exit 1; \
	done
	@$(MAKE) --no-print-directory -C test check
	@typos

.PHONY: clean
clean:
	@echo "Cleaning up Asterinas workspace target files"
	@cargo clean
	@echo "Cleaning up OSDK workspace target files"
	@cd osdk && cargo clean
	@echo "Cleaning up documentation target files"
	@cd docs && mdbook clean
	@echo "Cleaning up test target files"
	@$(MAKE) --no-print-directory -C test clean
	@echo "Uninstalling OSDK"
	@rm -f $(CARGO_OSDK)

# 将编译生成的 ELF 可执行文件复制到项目根目录，命名为 `kernel-rv`
.PHONY: kernel-rv
kernel-rv: build
	@cp target/osdk/aster-nix-osdk-bin.qemu_elf kernel-rv
