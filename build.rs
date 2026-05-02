use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

// Build FLINT, install it into OUT_DIR, then prepare Rust bindings.

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

struct Build {
    out_dir: PathBuf,
    bindings_rs: PathBuf,
    flint_include_dir: PathBuf,
    flint_lib_dir: PathBuf,
    flint_install_dir: Option<PathBuf>,
}

impl Build {
    fn new() -> Result<Self> {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap())
            .canonicalize()
            .unwrap();

        let flint_install_dir = flint_install_dir()?;
        let (flint_include_dir, flint_lib_dir) = if let Some(prefix) = &flint_install_dir {
            (prefix.join("include"), prefix.join("lib"))
        } else {
            (out_dir.join("include"), out_dir.join("lib"))
        };

        Ok(Build {
            out_dir: out_dir.clone(),
            bindings_rs: out_dir.join("flint.rs"),
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

        // FLINT does not support out-of-tree compilation.
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
                .arg("--prefix") // ask that the files are install in OUT_DIR, not `/usr`
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
    println!("cargo::rerun-if-changed=flint-install");

    if let Some(path) = std::env::var_os("FLINT_INSTALL") {
        anyhow::ensure!(!os_str_is_empty(&path), "`FLINT_INSTALL` is set but empty");
        let prefix = PathBuf::from(path);
        return validate_flint_install_dir(&prefix)
            .with_context(|| format!("Invalid `FLINT_INSTALL={}`", prefix.display()))
            .map(Some);
    }

    let prefix = Path::new("flint-install");
    if std::fs::symlink_metadata(prefix).is_ok() {
        return validate_flint_install_dir(prefix)
            .context("Invalid `flint-install` FLINT install prefix")
            .map(Some);
    }

    Ok(None)
}

fn os_str_is_empty(value: &OsStr) -> bool {
    value.as_encoded_bytes().is_empty()
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

// Copy pregenerated bindings for normal crate builds.
#[cfg(not(feature = "force-bindgen"))]
impl Build {
    fn prepare_bindings(&self) -> Result<()> {
        println!("cargo::rerun-if-changed=./bindgen/flint.rs");
        std::fs::copy("./bindgen/flint.rs", &self.bindings_rs)
            .context("Failed to copy pregenerated bindings")?;
        Ok(())
    }
}

#[cfg(feature = "force-bindgen")]
impl Build {
    fn prepare_bindings(&self) -> Result<()> {
        anyhow::bail!("force-bindgen modular pipeline is not implemented yet")
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

    anyhow::ensure!(build.bindings_rs.is_file(), "Cannot find `flint.rs`");

    Ok(())
}
