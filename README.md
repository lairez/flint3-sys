# flint3-sys

[FLINT](https://flintlib.org/) bindings for the Rust programming language, using [bindgen](https://github.com/rust-lang/rust-bindgen).

Since the FLINT API evolves quickly, this crate normally compiles the bundled FLINT from source. A system FLINT can be used explicitly with the `use-system-lib` feature.

## Versioning

This crate follows FLINT's versioning, except for the patch version, which may increase faster.


## Optional Features

- `gmp-mpfr-sys`: Enables a dependency on the [gmp-mpfr-sys](https://crates.io/crates/gmp-mpfr-sys) crate and builds FLINT against GMP and MPFR libraries compiled by it. If this feature is **not** enabled, there is a system dependency on GMP and MPFR.

- `run-bindgen`: Adds [bindgen](https://github.com/rust-lang/rust-bindgen) as a build dependency and regenerates the bindings. Useful for maintenance (see below) or for using system libraries.

- `use-system-lib`: Links dynamically against a system FLINT discovered with `pkg-config`. This uses the checked-in bindings unless `run-bindgen` is also enabled. The detected FLINT major/minor version must match the bundled FLINT version when using checked-in bindings.

## Metadata

This crate passes the following metadata to its dependents:

- `DEP_FLINT_LIB_DIR`: Path to the directory containing FLINT, when available.
- `DEP_FLINT_INCLUDE_DIR`: Path to the directory containing FLINT headers, such as `DEP_FLINT_INCLUDE_DIR/flint/flint.h`.

This is useful for crates that need to compile a C library depending on FLINT. See the [Cargo book](https://doc.rust-lang.org/cargo/reference/build-scripts.html#the-links-manifest-key) for more details.

## System FLINT

To use a system FLINT instead of building the bundled source:

```sh
cargo test --features use-system-lib
```

This requires a `flint.pc` file visible to `pkg-config`. The system FLINT is linked dynamically. Do not combine this feature with `gmp-mpfr-sys`; the system FLINT package already determines its GMP and MPFR linkage.

## Licensing

This crate is distributed under the MIT license.  
Note that FLINT itself is licensed under the LGPLv3.


## Why not [flint-sys](https://crates.io/crates/flint-sys)?

~~There seems to be too much manual work which makes the maintenance hard and the package often outdated.~~

That was the reason, but `flint-sys` is now up-to-date.
Choose what works for you.

## Architecture

Creating a `*-sys` crate involves several design decisions, as outlined by [Kornel](https://kornel.ski/rust-sys-crate).

### Which FLINT library?

By default, this crate builds the bundled FLINT source and links it statically. This is slower than linking to an already installed library, but it gives the crate control over the FLINT version and over the headers used to generate `bindgen/flint.rs`.

The `use-system-lib` feature takes the opposite tradeoff: it asks `pkg-config` for an installed FLINT and links to it dynamically. This is convenient for distributions and for users who already manage FLINT themselves, but it means that the headers and the library are no longer controlled by this crate.

FLINT's C API is not as stable as a typical system library ABI. For that reason, `use-system-lib` checks that the system FLINT major/minor version matches the bundled FLINT version when using the checked-in bindings. A patch-level difference is accepted; a different major/minor version is rejected unless you also enable `run-bindgen`.

### When to run bindgen?

There are two strategies:

1. Generate bindings during maintenance and ship `bindgen/flint.rs`.
2. Run `bindgen` in the build script against the headers found on the build machine.

The default is to use the checked-in bindings. This keeps normal builds faster and avoids requiring `libclang`. It also gives downstream users a predictable Rust API.

The `run-bindgen` feature regenerates the bindings during the build. This is useful when updating the bundled FLINT version, or when deliberately binding to system headers. It is not the default because :

- It is slower (since *bindgen* is not so fast),
- It requires `libclang`,
- It a Rust API that depends on the installed FLINT headers,

### GMP and MPFR

FLINT depends on GMP and MPFR. With the bundled FLINT build, this crate can either rely on system GMP/MPFR or, if the `gmp-mpfr-sys` feature is enabled, build and link against the copies provided by the `gmp-mpfr-sys` crate.

The `gmp-mpfr-sys` feature is intentionally incompatible with `use-system-lib`. A system FLINT package has already been built against some GMP and MPFR libraries, and mixing that FLINT with a different Rust-built GMP/MPFR pair would be fragile.

This crate does not expose GMP or MPFR APIs directly; they are only FLINT dependencies.

### Build script pipeline

The build script first decides where FLINT comes from:

- Without `use-system-lib`, it configures, builds, and installs the bundled FLINT into Cargo's `OUT_DIR`, then links `libflint.a` statically.
- With `use-system-lib`, it asks `pkg-config` for include paths, link paths, and dynamic link flags, and does not build bundled FLINT.

Then it provides Rust bindings:

- Without `run-bindgen`, it copies the checked-in `bindgen/flint.rs` into `OUT_DIR`.
- With `run-bindgen`, it regenerates `flint.rs` from the active FLINT headers.


## Maintenance

To update the bundled version of FLINT:

1. Update the `./flint` submodule.
2. Run `KEEP_BINDGEN_OUTPUT=1 cargo build -F run-bindgen` to regenerate `./bindgen/flint.rs`.
3. Test thoroughly.
4. Commit the changes.
