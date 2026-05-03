# flint3-sys

[FLINT](https://flintlib.org/) bindings for the Rust programming language, using [bindgen](https://github.com/rust-lang/rust-bindgen).

## Versioning

This crate follows FLINT's versioning, except for the patch version, which may increase faster.


## Optional Features

- `gmp-mpfr-sys`: Enables a dependency on the [gmp-mpfr-sys](https://crates.io/crates/gmp-mpfr-sys) crate and builds bundled FLINT against GMP and MPFR libraries provided by it. If this feature is *not* enabled, the bundled FLINT build relies on system GMP and MPFR.

- `run-bindgen`: Adds [bindgen](https://github.com/rust-lang/rust-bindgen) as a build dependency and regenerates the bindings. Useful for maintenance (see below) or for using system libraries.

- `use-system-libs`: Links dynamically against a system FLINT discovered with `pkg-config`, and enables `gmp-mpfr-sys/use-system-libs` so GMP and MPFR are also treated as system libraries when that crate participates. This uses the checked-in bindings unless `run-bindgen` is also enabled. The detected FLINT major/minor version must match the bundled FLINT version when using checked-in bindings.

  This feature is experimental and will probably remain so forever. The reason is that we cannot provide a stable Rust API linking against an unknown system library. But this sometimes work and it cuts the compilation time significantly.

## Metadata

This crate passes the following metadata to its dependents:

- `DEP_FLINT_LIB_DIR`: Path to the directory containing FLINT, when available.
- `DEP_FLINT_INCLUDE_DIR`: Path to the directory containing FLINT headers, such as `DEP_FLINT_INCLUDE_DIR/flint/flint.h`.

This is useful for crates that need to compile a C library depending on FLINT. See the [Cargo book](https://doc.rust-lang.org/cargo/reference/build-scripts.html#the-links-manifest-key) for more details.

## System FLINT

To use a system FLINT instead of building the bundled source:

```sh
cargo test --features use-system-libs
```

This requires a `flint.pc` file visible to `pkg-config`. The system FLINT is linked dynamically. This feature also forwards `use-system-libs` to `gmp-mpfr-sys`, so the dependency graph consistently uses system FLINT, GMP, and MPFR instead of mixing a system FLINT with Rust-built GMP/MPFR libraries.

## Licensing

This crate is distributed under the MIT license.  
Note that FLINT itself is licensed under the LGPLv3.


## Why not [flint-sys](https://crates.io/crates/flint-sys)?

~~There seems to be too much manual work which makes the maintenance hard and the package often outdated.~~

That was the reason, but `flint-sys` is now up-to-date.
Choose what works for you.

## Architecture

Creating a `*-sys` crate involves several design decisions, as outlined by [Kornel](https://kornel.ski/rust-sys-crate). And there is no perfect choice.

### Which FLINT library?

By default, this crate builds the bundled FLINT source and links it statically.

  - :green_circle: We control FLINT's version
  - :green_circle: FLINT is compiled with applicable hardware optimizations
  - :red_circle: Compilation time
  
The `use-system-libs` feature takes the opposite approach: it asks `pkg-config` for an installed FLINT and links to it dynamically. It checks that the system FLINT major/minor version matches the bundled FLINT version when using the checked-in bindings. A patch-level difference is accepted; a different majoer/minor version is rejected unless you also enable `run-bindgen`.

  - :green_circle: No need to compile FLINT
  - :red_circle: The system FLINT version may not match the API provided by `bindgen/flint.rs`
  
### When to run bindgen?

There are two strategies:

1. Generate bindings during maintenance and ship `bindgen/flint.rs`.
2. Run `bindgen` in the build script.

The default is to use the checked-in bindings. This keeps normal builds faster and avoids requiring `libclang`. It also gives downstream users a predictable Rust API. But *it is important to understand that the bindings may not be 100% accurate!* The FLINT API depends on *both* the version *and* the machine-dependent configuration. We assume `FLINT_BITS==64` and `FLINT_USES_PTHREAD`, but we don't assume anything on `FLINT_USES_CPUSET`, this is why `thread_pool.h` is not exposed.

The `run-bindgen` feature regenerates the bindings during the build. This ensure that the bindings are accurate, but this means that the Rust API is not fully predictable. (This feature is also useful for the package maintenance.)

### Concretely

- *default features*. Uses the bundled FLINT library and the bundled bindings.
    - :green_circle: Predictable FLINT version
    - :green_circle: Predictable Rust API
    - :red_circle: Large compile time
    
- `--feature use-system-libs`. Uses the system FLINT library and the bundled bindings. You can use this option on your computer if you know what you are doing.
    - :green_circle: Fast compilation
    - :red_circle: Unpredictable FLINT version
    - :orange_circle: The Rust API may not match the system FLINT, but it may still be OK if only the minor version differs.

- `--feature run-bindgen`. Uses the bundled FLINT library and generate the bindings at build time. This is especially useful when the maintainer update the bundled FLINT library.
    - :green_circle: Predictable FLINT version
    - :green_circle: Predictable Rust API
    - :red_circle: Large compile time
    - :red_circle: Dependence on `bindgen` and `libclang`
    
- `--feature run-bindgen,use-system-libs`. Uses the system FLINT library and generate the bindings at build time.
    - :red_circle: Unpredictable FLINT version
    - :red_circle: Unpredictable Rust API
    - :red_circle: The binding generation is a fragil process, on an unknown FLINT version, expect the unexpected
    - :green_circle: If it works, Rust API and FLINT will be compatible
    - :green_circle: Fast compilation


### GMP and MPFR

FLINT depends on GMP and MPFR. With the bundled FLINT build, this crate can either rely on system GMP/MPFR or, if the `gmp-mpfr-sys` feature is enabled, build and link against the libraries provided by the `gmp-mpfr-sys` crate.

The feature `use-system-libs` also enables `gmp-mpfr-sys/use-system-libs`. *Recall that this feature is experimental.*


## Maintenance

To update the bundled version of FLINT:

1. Update the `./flint` submodule.
2. Run `KEEP_BINDGEN_OUTPUT=1 cargo build -F run-bindgen` to regenerate `./bindgen/flint.rs`.
3. Test thoroughly.
4. Commit the changes.
