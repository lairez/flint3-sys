use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

// Build or locate FLINT, then provide `flint.rs` to the crate.
//
// Normal builds copy the checked-in bindings from `bindgen/flint.rs`.
// `--features force-bindgen` regenerates that file by running bindgen once per
// FLINT header. Per-header bindgen is much faster than a single mega-header, but
// it requires a few explicit policies below to avoid duplicate declarations.

const GENERATED_BINDINGS: &str = "bindgen/flint.rs";

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

// Temporary workaround for FLINT headers where `mpoly_void_ring_t` is an
// array typedef over an anonymous struct. Bindgen then emits unstable
// `_bindgen_ty_*` names in unrelated headers. Naming the struct gives
// bindgen a stable Rust type. Remove this when upstream FLINT has the fix.
fn patch_flint_mpoly_void_ring_type(header: &Path) -> Result<()> {
    println!("cargo::rerun-if-changed={}", header.display());
    let source = std::fs::read_to_string(header)
        .with_context(|| format!("Failed to read `{}`", header.display()))?;

    if source.contains("mpoly_void_ring_struct") {
        return Ok(());
    }

    let patched = source.replace(
        "} mpoly_void_ring_t[1];",
        "} mpoly_void_ring_struct;\n\ntypedef mpoly_void_ring_struct mpoly_void_ring_t[1];",
    );
    anyhow::ensure!(
        patched != source,
        "Could not find anonymous `mpoly_void_ring_t` declaration in `{}`",
        header.display()
    );

    std::fs::write(header, patched)
        .with_context(|| format!("Failed to patch `{}`", header.display()))?;

    Ok(())
}

// Paths selected once at startup and shared by the build and binding phases.
struct Build {
    // Cargo build-script scratch directory.
    out_dir: PathBuf,
    // Destination consumed by src/lib.rs through include!(env!("FLINT_RS")).
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
            // `FLINT_INSTALL` or `flint-install` points to a complete install
            // prefix. We still patch the installed header so force-bindgen sees
            // the same workaround as vendored builds.
            patch_flint_mpoly_void_ring_type(&self.flint_include_dir.join("flint/mpoly_types.h"))?;
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

        patch_flint_mpoly_void_ring_type(&flint_root_dir.join("src/mpoly_types.h"))?;

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
static SKIP_HEADERS: &[&str] = &[
    // Headers that are platform/configuration internals, test helpers, or pull
    // in dependencies we intentionally do not expose yet.
    r"^NTL-interface\.h$",
    r"^config\.h$",
    r"^flint-config\.h$",
    r"^flint-mparam\.h$",
    r"^crt_helpers\.h$",
    r"^gettimeofday\.h$",
    r"^gmpcompat\.h$",
    r"^longlong.*\.h$",
    r"^fft_small\.h$",
    r"^fft\.h$",
    r"^machine_vectors\.h$",
    r"^mpn_extras\.h$",
    r"^profiler\.h$",
    r"^test_helpers\.h$",
    r"^.*templates\.h$",
];

#[cfg(feature = "force-bindgen")]
static SKIP_ITEMS: &[(&str, &[&str])] = &[
    // Some public headers repeat declarations from other headers. Keep this as
    // a small hand-curated list rather than adding fragile global dedup logic.
    (
        "flint.h",
        &["n_randlimb", "n_randtest", "n_randtest_not_zero"],
    ),
    (
        "gr_generic.h",
        &[
            "gr_generic_ctx_predicate",
            "gr_generic_ctx_predicate_true",
            "gr_generic_ctx_predicate_false",
        ],
    ),
    ("mpn_mod.h", &["gr_ctx_init_mpn_mod"]),
];

#[cfg(feature = "force-bindgen")]
impl Build {
    fn flint_headers(&self) -> Result<Vec<PathBuf>> {
        let flint_header_dir = self.flint_include_dir.join("flint");
        anyhow::ensure!(
            flint_header_dir.is_dir(),
            "Cannot find FLINT header directory `{}`",
            flint_header_dir.display()
        );

        let skip_patterns = SKIP_HEADERS
            .iter()
            .map(|pattern| regex::Regex::new(pattern))
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Invalid SKIP_HEADERS regex")?;

        let mut headers = Vec::new();
        for entry in std::fs::read_dir(&flint_header_dir)
            .with_context(|| format!("Failed to read `{}`", flint_header_dir.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("h") {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
                continue;
            };

            if skip_patterns.iter().any(|re| re.is_match(file_name)) {
                continue;
            }
            headers.push(path);
        }

        headers.sort();
        headers.dedup();
        Ok(headers)
    }

    fn prepare_bindings(&self) -> Result<()> {
        use std::io::Write;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let headers = self.flint_headers()?;

        // FLINT implements many inline functions by compiling module-specific
        // `inlines.c` files into libflint. Defining the matching `*_INLINES_C`
        // macros tells headers to expose those functions as external
        // declarations, so bindgen can generate Rust bindings without creating
        // wrapper C files.
        let inline_macro_pattern =
            regex::Regex::new(r"(?m)^\s*#\s*ifdef\s+([A-Z0-9_]+_INLINES_C)\b")
                .context("Invalid inline macro regex")?;
        let mut inline_macros = Vec::new();
        for header in &headers {
            let source = std::fs::read_to_string(header)
                .with_context(|| format!("Failed to read `{}`", header.display()))?;
            inline_macros.extend(
                inline_macro_pattern
                    .captures_iter(&source)
                    .map(|captures| captures[1].to_owned()),
            );
        }
        inline_macros.sort();
        inline_macros.dedup();

        let _flint_rs = std::fs::File::create(&self.flint_rs)?;
        let mut flint_rs = std::io::BufWriter::new(_flint_rs);

        write!(flint_rs, "/*  automatically generated by rust-bindgen */")?;
        let header_strings = headers
            .iter()
            .map(|header| {
                let h = header.to_str().context("Non unicode header path")?;
                Ok(h.to_owned())
            })
            .collect::<Result<Vec<_>>>()?;

        let mut generated = std::iter::repeat_with(|| None)
            .take(header_strings.len())
            .collect::<Vec<Option<(String, String)>>>();
        let next_header = Arc::new(AtomicUsize::new(0));
        let worker_count = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(header_strings.len());

        // Bindgen is invoked once per header. This keeps each clang parse small
        // and makes the work easy to parallelize. The final file is assembled in
        // sorted header order, so output remains deterministic.
        std::thread::scope(|scope| -> Result<()> {
            let mut tasks = Vec::with_capacity(worker_count);
            for _ in 0..worker_count {
                let next_header = Arc::clone(&next_header);
                let header_strings = &header_strings;
                let inline_macros = &inline_macros;
                tasks.push(
                    scope.spawn(move || -> Result<Vec<(usize, String, String)>> {
                        let mut generated = Vec::new();
                        loop {
                            let index = next_header.fetch_add(1, Ordering::Relaxed);
                            let Some(h) = header_strings.get(index).cloned() else {
                                break;
                            };

                            println!("cargo::rerun-if-changed={h}");
                            let header_name = Path::new(&h)
                                .file_name()
                                .and_then(std::ffi::OsStr::to_str)
                                .context("Header path has no file name")?;

                            let mut builder = bindgen::Builder::default()
                                .clang_arg("-DFLINT_NOSTDIO")
                                .clang_arg("-DFLINT_NOSTDARG")
                                .disable_header_comment()
                                .header(&h)
                                .allowlist_file(regex::escape(&h))
                                .allowlist_recursively(false)
                                .blocklist_var(".*")
                                .blocklist_function(".*_mpn.*")
                                .blocklist_function(".*_mpz.*")
                                .derive_default(true)
                                .derive_copy(false)
                                .derive_debug(false)
                                .default_non_copy_union_style(
                                    bindgen::NonCopyUnionStyle::ManuallyDrop,
                                )
                                .generate_cstr(true)
                                .merge_extern_blocks(true)
                                .rust_target(bindgen::RustTarget::stable(82, 0).ok().unwrap())
                                .rust_edition(bindgen::RustEdition::Edition2021)
                                .layout_tests(false)
                                .formatter(bindgen::Formatter::Prettyplease);

                            for inline_macro in inline_macros {
                                builder = builder.clang_arg(format!("-D{inline_macro}"));
                            }

                            for (_, items) in SKIP_ITEMS
                                .iter()
                                .filter(|(header, _)| *header == header_name)
                            {
                                for item in *items {
                                    builder = builder.blocklist_function(format!("^{item}$"));
                                }
                            }
                            let bindings = builder
                                .generate()
                                .context("Failed to generate FLINT type bindings")?;
                            let stem = Path::new(&h)
                                .file_stem()
                                .and_then(std::ffi::OsStr::to_str)
                                .context("Header path has no file stem")?
                                .to_owned();

                            // Each bindgen run starts anonymous names at
                            // `_bindgen_ty_1`. Prefix them with the header stem
                            // before concatenating all generated fragments.
                            generated.push((
                                index,
                                h,
                                bindings
                                    .to_string()
                                    .replace("_bindgen_", &format!("_{stem}_bindgen_")),
                            ));
                        }
                        Ok(generated)
                    }),
                );
            }

            for task in tasks {
                for (index, h, bindings) in task.join().expect("bindgen worker panicked")? {
                    generated[index] = Some((h, bindings));
                }
            }
            Ok(())
        })?;

        for entry in generated {
            let (h, bindings) = entry.context("Missing generated bindings")?;
            let file_name = std::path::Path::new(&h)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or(&h);
            write!(flint_rs, "\n\n/* {} */\n\n{}", file_name, bindings)?;
        }

        println!("cargo::rerun-if-env-changed=KEEP_BINDGEN_OUTPUT");
        if std::env::var_os("KEEP_BINDGEN_OUTPUT").is_some() {
            std::fs::copy(&self.flint_rs, GENERATED_BINDINGS)
                .context("Failed to copy generated bindings")?;
        }

        Ok(())
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
