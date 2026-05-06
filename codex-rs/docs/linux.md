To build for Linux from macOS, you have two main options:

  Option 1: Cross-compile with cross (easiest)

  cargo install cross
  cross build --target x86_64-unknown-linux-gnu

  cross uses Docker under the hood — no manual toolchain setup needed.

  Option 2: Native cross-compile with rustup

  rustup target add x86_64-unknown-linux-gnu
  brew install FiloSottile/musl-cross/musl-cross
  CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-musl-gcc cargo build --target x86_64-unknown-linux-gnu

  This produces a statically-linked musl binary that runs on virtually any Linux distro.

  Recommendation: Use cross if you have Docker. It handles the linker and C dependencies automatically. Use the musl approach if you want no Docker dependency and a portable static binary.

  If you need ARM Linux (e.g. Raspberry Pi or AWS Graviton), swap the target for aarch64-unknown-linux-gnu.
