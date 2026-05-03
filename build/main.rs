use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

#[cfg(feature = "force-bindgen")]
mod runbindgen;

// Build or locate FLINT, then provide `flint.rs` to the crate.
//
// Normal builds copy the checked-in bindings from `bindgen/flint.rs`.
// `--features force-bindgen` regenerates that file by running bindgen once per
// FLINT header. Per-header bindgen is much faster than a single mega-header, but
// it requires a few explicit policies below to avoid duplicate declarations.

// This is an undocumented convenience for the maintainer of this crate, so that
// FLINT is not compiled over and over.
const LOCAL_FLINT_INSTALL: &str = "flint-install";

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
    // `bindgen/flint.rs`.
    flint_rs: PathBuf,
    // FLINT include prefix, either OUT_DIR/include or <FLINT_INSTALL>/include.
    flint_include_dir: PathBuf,
    // FLINT library prefix, either OUT_DIR/lib or <FLINT_INSTALL>/lib.
    flint_lib_dir: PathBuf,
    // Existing FLINT install prefix. None means build bundled FLINT in OUT_DIR.
    flint_install_dir: Option<PathBuf>,
}

impl Build {
    fn new() -> Result<Self> {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").context("Missing OUT_DIR")?)
            .canonicalize()
            .context("Failed to canonicalize OUT_DIR")?;

        // Prefer an existing FLINT install while developing. Otherwise build and
        // install the bundled FLINT into OUT_DIR.
        let flint_install_dir = flint_install_dir()?;
        let (flint_include_dir, flint_lib_dir) = if let Some(prefix) = &flint_install_dir {
            (prefix.join("include"), prefix.join("lib"))
        } else {
            (out_dir.join("include"), out_dir.join("lib"))
        };

        Ok(Build {
            out_dir: out_dir.clone(),
            flint_rs: out_dir.join("flint.rs"),
            flint_include_dir,
            flint_lib_dir,
            flint_install_dir,
        })
    }

    fn build_flint(&self) -> Result<()> {
        if self.flint_install_dir.is_some() {
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
        println!("cargo::metadata=LIB_DIR={}", self.flint_lib_dir.display());
        println!(
            "cargo::metadata=INCLUDE_DIR={}",
            self.flint_include_dir.display()
        );
    }
}

fn flint_install_dir() -> Result<Option<PathBuf>> {
    println!("cargo::rerun-if-env-changed=FLINT_INSTALL");
    println!("cargo::rerun-if-changed={LOCAL_FLINT_INSTALL}");

    if let Some(path) = std::env::var_os("FLINT_INSTALL") {
        anyhow::ensure!(
            !path.as_encoded_bytes().is_empty(),
            "`FLINT_INSTALL` is set but empty"
        );
        let prefix = PathBuf::from(path);
        return validate_flint_install_dir(&prefix)
            .with_context(|| format!("Invalid `FLINT_INSTALL={}`", prefix.display()))
            .map(Some);
    }

    let prefix = Path::new(LOCAL_FLINT_INSTALL);
    if std::fs::symlink_metadata(prefix).is_ok() {
        return validate_flint_install_dir(prefix)
            .context("Invalid `flint-install` FLINT install prefix")
            .map(Some);
    }

    Ok(None)
}

fn validate_flint_install_dir(prefix: &Path) -> Result<PathBuf> {
    anyhow::ensure!(
        prefix.is_dir(),
        "`{}` is not a directory, or is a broken symlink",
        prefix.display()
    );

    let include_header = prefix.join("include/flint/flint.h");
    let static_lib = prefix.join("lib/libflint.a");

    anyhow::ensure!(
        include_header.is_file(),
        "Missing `{}`",
        include_header.display()
    );
    anyhow::ensure!(static_lib.is_file(), "Missing `{}`", static_lib.display());

    println!("cargo::rerun-if-changed={}", include_header.display());
    println!("cargo::rerun-if-changed={}", static_lib.display());

    prefix
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize `{}`", prefix.display()))
}

// Normal crate builds must not require libclang/bindgen. They use the checked-in
// file generated by `KEEP_BINDGEN_OUTPUT=1 cargo build --features force-bindgen`.
#[cfg(not(feature = "force-bindgen"))]
impl Build {
    fn prepare_bindings(&self) -> Result<()> {
        println!("cargo::rerun-if-changed={GENERATED_BINDINGS}");
        std::fs::copy(GENERATED_BINDINGS, &self.flint_rs)
            .context("Failed to copy pregenerated bindings")?;

        Ok(())
    }
}

#[cfg(feature = "force-bindgen")]
impl Build {
    fn prepare_bindings(&self) -> Result<()> {
        let bgen = runbindgen::BindingGeneration::new(self.flint_include_dir.clone(), self.flint_rs.clone())?;
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

    anyhow::ensure!(
        build.flint_lib_dir.join("libflint.a").is_file(),
        "Compilation is successful, but `libflint.a` is not where it should"
    );

    println!("cargo::rustc-link-lib=flint");
    println!("cargo::rustc-link-lib=mpfr");
    println!("cargo::rustc-link-lib=gmp");
    println!(
        "cargo::rustc-link-search=native={}",
        build.flint_lib_dir.display()
    );

    if cfg!(feature = "gmp-mpfr-sys") {
        println!(
            "cargo::rustc-link-search=native={}",
            std::env::var("DEP_GMP_LIB_DIR")?
        );
    }

    build.prepare_bindings()?;

    anyhow::ensure!(build.flint_rs.is_file(), "Cannot find `flint.rs`");

    Ok(())
}
