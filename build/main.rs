use std::{path::PathBuf, process::Command};

use anyhow::{Context, Result};

#[cfg(feature = "run-bindgen")]
mod runbindgen;

// Build or locate FLINT, then provide `flint.rs` to the crate.
//
// Normal builds copy the checked-in bindings from `bindgen/flint.rs`.
// `--features run-bindgen` regenerates that file by running bindgen once per
// FLINT header. Per-header bindgen is much faster than a single mega-header, but
// it requires a few explicit policies below to avoid duplicate declarations.

#[cfg(not(feature = "run-bindgen"))]
const GENERATED_BINDINGS: &str = "bindgen/flint.rs";

fn run(mut command: Command) -> Result<()> {
    let command_string = format!("{command:?}");

    let output = command
        .output()
        .with_context(|| format!("Command {command_string} did not execute normally"))?;

    if !output.status.success() {
        anyhow::bail!(
            "Command failed\nCommand: {}\n===== stdout\n{}===== stderr\n{}\n",
            command_string,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    }

    Ok(())
}

// Paths selected once at startup and shared by the build and binding phases.
struct Build {
    // Cargo build-script scratch directory.
    out_dir: PathBuf,
    // Destination consumed by src/lib.rs through include!(env!("FLINT_RS")).
    flint_rs: PathBuf,
    // FLINT include prefix, either OUT_DIR/include or a system include path.
    flint_include_dir: PathBuf,
    // FLINT library prefix for bundled static builds. System builds use pkg-config.
    flint_lib_dir: Option<PathBuf>,
    link: LinkMode,
}

enum LinkMode {
    BundledStatic,
    SystemDynamic,
}

impl Build {
    fn new() -> Result<Self> {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").context("Missing OUT_DIR")?)
            .canonicalize()
            .context("Failed to canonicalize OUT_DIR")?;

        let (flint_include_dir, flint_lib_dir, link) = if cfg!(feature = "use-system-libs") {
            let system = system_flint()?;
            (system.include_dir, system.lib_dir, LinkMode::SystemDynamic)
        } else {
            (
                out_dir.join("include"),
                Some(out_dir.join("lib")),
                LinkMode::BundledStatic,
            )
        };

        Ok(Build {
            out_dir: out_dir.clone(),
            flint_rs: out_dir.join("flint.rs"),
            flint_include_dir,
            flint_lib_dir,
            link,
        })
    }

    fn build_flint(&self) -> Result<()> {
        if matches!(self.link, LinkMode::SystemDynamic) {
            self.emit_flint_metadata();
            return Ok(());
        }

        let flint_root_dir = self.out_dir.join("flint");
        let tmp_dir = self.out_dir.join("tmp");
        std::fs::create_dir_all(&tmp_dir)
            .context(format!("Failed to create `{}`", tmp_dir.display()))?;

        // FLINT does not support out-of-tree compilation, so Cargo builds a
        // private copy under OUT_DIR.
        let mut cp = Command::new("cp");
        cp.arg("-Rp").arg("flint").arg(&self.out_dir);
        run(cp)?;

        if !flint_root_dir.join("configure").is_file() {
            let mut bootstrap = Command::new("sh");
            bootstrap
                .current_dir(&flint_root_dir)
                .env("TMPDIR", &tmp_dir)
                .arg("./bootstrap.sh");

            run(bootstrap)?;
        }

        if !flint_root_dir.join("Makefile").is_file() {
            let mut configure = Command::new("sh");

            configure
                .current_dir(&flint_root_dir)
                .env("TMPDIR", &tmp_dir)
                .arg("./configure")
                .arg("--prefix")
                .arg(&self.out_dir)
                .arg("--disable-shared");

            if cfg!(feature = "gmp-mpfr-sys") {
                configure
                    .arg(format!(
                        "--with-gmp-lib={}",
                        std::env::var("DEP_GMP_LIB_DIR")?
                    ))
                    .arg(format!(
                        "--with-gmp-include={}",
                        std::env::var("DEP_GMP_INCLUDE_DIR")?
                    ));
            }

            run(configure)?;
        }

        if !flint_root_dir.join("libflint.a").is_file() {
            let mut make = Command::new("make");
            make.current_dir(&flint_root_dir);
            make.env("TMPDIR", &tmp_dir);
            make.env("MAKEFLAGS", std::env::var("CARGO_MAKEFLAGS").unwrap());
            run(make)?;
        }

        let mut make_install = Command::new("make");
        make_install.current_dir(&flint_root_dir).arg("install");
        make_install.env("TMPDIR", &tmp_dir);

        run(make_install)?;

        self.emit_flint_metadata();

        Ok(())
    }

    fn emit_flint_metadata(&self) {
        if let Some(flint_lib_dir) = &self.flint_lib_dir {
            println!("cargo::metadata=LIB_DIR={}", flint_lib_dir.display());
        }
        println!(
            "cargo::metadata=INCLUDE_DIR={}",
            self.flint_include_dir.display()
        );
    }
}

struct SystemFlint {
    include_dir: PathBuf,
    lib_dir: Option<PathBuf>,
}

#[cfg(feature = "use-system-libs")]
fn system_flint() -> Result<SystemFlint> {
    let library = pkg_config::Config::new()
        .statik(false)
        .probe("flint")
        .context("Failed to find system FLINT with pkg-config")?;

    if !cfg!(feature = "run-bindgen") {
        validate_system_flint_version(&library.version)?;
    }

    let include_dir = library
        .include_paths
        .iter()
        .find(|path| path.join("flint/flint.h").is_file())
        .cloned()
        .context("pkg-config did not report an include path containing `flint/flint.h`")?;
    let lib_dir = library.link_paths.first().cloned();

    println!("cargo::metadata=SYSTEM_LIB=1");
    println!("cargo::rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo::rerun-if-env-changed=PKG_CONFIG_LIBDIR");
    println!("cargo::rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");

    Ok(SystemFlint {
        include_dir,
        lib_dir,
    })
}

#[cfg(not(feature = "use-system-libs"))]
fn system_flint() -> Result<SystemFlint> {
    unreachable!("system_flint is only called with `use-system-libs`")
}

#[cfg(feature = "use-system-libs")]
fn validate_system_flint_version(version: &str) -> Result<()> {
    println!("cargo::rerun-if-changed=flint/VERSION");
    let bundled_version =
        std::fs::read_to_string("flint/VERSION").context("Failed to read `flint/VERSION`")?;
    let bundled_version = bundled_version.trim();
    let bundled_major_minor = major_minor(bundled_version)
        .with_context(|| format!("Could not parse bundled FLINT version `{bundled_version}`"))?;
    let system_major_minor = major_minor(version)
        .with_context(|| format!("Could not parse system FLINT version `{version}`"))?;

    anyhow::ensure!(
        bundled_major_minor == system_major_minor,
        "System FLINT version `{}` is incompatible with checked-in bindings for FLINT {}.x; \
         install a matching FLINT or enable `run-bindgen`",
        version,
        bundled_major_minor
    );

    Ok(())
}

#[cfg(feature = "use-system-libs")]
fn major_minor(version: &str) -> Option<String> {
    let mut parts = version.split('.');
    Some(format!("{}.{}", parts.next()?, parts.next()?))
}

// Normal crate builds must not require libclang/bindgen. They use the checked-in
// file generated by `KEEP_BINDGEN_OUTPUT=1 cargo build --features run-bindgen`.
#[cfg(not(feature = "run-bindgen"))]
impl Build {
    fn prepare_bindings(&self) -> Result<()> {
        println!("cargo::rerun-if-changed={GENERATED_BINDINGS}");
        std::fs::copy(GENERATED_BINDINGS, &self.flint_rs)
            .context("Failed to copy pregenerated bindings")?;

        Ok(())
    }
}

#[cfg(feature = "run-bindgen")]
impl Build {
    fn prepare_bindings(&self) -> Result<()> {
        let bgen = runbindgen::BindingGeneration::new(
            self.flint_include_dir.clone(),
            self.flint_rs.clone(),
        )?;
        bgen.generate_bindings()
    }
}

fn main() -> Result<()> {
    let build = Build::new()?;

    build.build_flint()?;

    anyhow::ensure!(
        build.flint_include_dir.join("flint/flint.h").is_file(),
        "Compilation is successful, but `flint/flint.h` is not where it should"
    );

    if matches!(build.link, LinkMode::BundledStatic) {
        let flint_lib_dir = build
            .flint_lib_dir
            .as_ref()
            .context("Missing bundled FLINT library directory")?;
        anyhow::ensure!(
            flint_lib_dir.join("libflint.a").is_file(),
            "Compilation is successful, but `libflint.a` is not where it should"
        );

        println!("cargo::rustc-link-lib=flint");
        println!("cargo::rustc-link-lib=mpfr");
        println!("cargo::rustc-link-lib=gmp");
        println!(
            "cargo::rustc-link-search=native={}",
            flint_lib_dir.display()
        );
    }

    if cfg!(feature = "gmp-mpfr-sys") {
        if let Ok(gmp_lib_dir) = std::env::var("DEP_GMP_LIB_DIR") {
            println!("cargo::rustc-link-search=native={gmp_lib_dir}");
        }
    }

    build.prepare_bindings()?;

    anyhow::ensure!(build.flint_rs.is_file(), "Cannot find `flint.rs`");

    Ok(())
}
