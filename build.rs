use std::{path::PathBuf, process::Command};

use anyhow::{Context, Result};

// WHAT?? More that 400 lines to build FLINT?. Hopefully every line will be clear after a few explanations.
//
// The pipeline is simple :
//
// STEP 1. Build FLINT, with the usual procedure (./configure, make, make install)
//
// STEP 2. Run bindgen, or simply copy the pregenerated files in the directory `./bindgen`
//
// NB: There is no need to compile the inline functions because FLINT already has a mechanism to include them in the binary library.

// Bindgen runs on a set of public FLINT entry headers. Some headers are useful
// only when included transitively, while others should not appear in the
// bindings at all.
#[allow(dead_code)]
static SKIP_BINDGEN_HEADERS: &[&str] = &[
    "NTL-interface.h",
    "config.h",
    "flint-config.h",
    "flint-mparam.h",
    "crt_helpers.h",
    "gettimeofday.h",
    "gmpcompat.h",
    "longlong.h",
    "longlong_asm_clang.h",
    "longlong_asm_gcc.h",
    "longlong_asm_gnu.h",
    "longlong_div_gnu.h",
    "longlong_msc_arm64.h",
    "longlong_msc_x86.h",
    "mpf-impl.h",
    "mpfr_mat.h",  // deprecated
    "mpfr_vec.h",  // deprecated
    "fft_small.h", // seems to cause some issues, but not so
    // important, fft_small is still used by FLINT even if the header
    // is not exposed.
    "machine_vectors.h", // idem
    "mpn_extras.h",
    "profiler.h",
    "test_helpers.h",
];

#[allow(dead_code)]
static SKIP_DIRECT_HEADERS: &[&str] = &[
    // Excluded from the bindings entirely.
    "NTL-interface.h",
    "config.h",
    "flint-config.h",
    "flint-mparam.h",
    "crt_helpers.h",
    "gettimeofday.h",
    "gmpcompat.h",
    "longlong.h",
    "longlong_asm_clang.h",
    "longlong_asm_gcc.h",
    "longlong_asm_gnu.h",
    "longlong_div_gnu.h",
    "longlong_msc_arm64.h",
    "longlong_msc_x86.h",
    "mpf-impl.h",
    "mpfr_mat.h",
    "mpfr_vec.h",
    "fft_small.h",
    "machine_vectors.h",
    "mpn_extras.h",
    "profiler.h",
    "test_helpers.h",
    // These are included by real public module headers. Passing them directly
    // makes clang parse extra translation units without adding entry points.
    "templates.h",
    "fq_templates.h",
    "fq_vec_templates.h",
    "fq_mat_templates.h",
    "fq_poly_templates.h",
    "fq_poly_factor_templates.h",
    "fq_embed_templates.h",
    // Type-only headers should be allowlisted but do not need to be entry
    // translation units.
    "acb_types.h",
    "acf_types.h",
    "arb_types.h",
    "arf_types.h",
    "ca_types.h",
    "fmpq_types.h",
    "fmpz_types.h",
    "fmpz_mod_types.h",
    "fq_types.h",
    "fq_nmod_types.h",
    "fq_zech_types.h",
    "gr_types.h",
    "limb_types.h",
    "mpoly_types.h",
    "n_poly_types.h",
    "nmod_types.h",
    "padic_types.h",
];

#[allow(dead_code)]
static BINDGEN_ALLOWLIST_FUNCTIONS: &[&str] = &[
    "^_?(acb|acf|aprcl|arb|arf|arith|bernoulli|bool|bsplit|butterfly|ca|calcium|clear|compute|d|di|dirichlet|dlog|double|evil|extract|fexpr|ff|fft|flint|fmpq|fmpz|fmpzi|fq|free|gr|hypgeom|ifft|insert|jacobi|long|mag|mpf|mpn|mpoly|mul|n|nf|nfixed|nfloat|nmod|pack|padic|parse|partitions|perm|poly|psl2z|qadic|qfb|qqbar|qs|qsieve|radix|reduce|slong|sp2gz|swap|thread|truth|tuple|ui|ulong|unity|z|zassenhaus)_.*",
    "^new_bitfield_.*",
];

#[allow(dead_code)]
static BINDGEN_ALLOWLIST_TYPES: &[&str] = &[
    "^_?(acb|acf|aprcl|arb|arf|bernoulli|bool|bsplit|ca|calcium|d|di|dirichlet|dlog|dot|fexpr|flint|fmpq|fmpz|fmpzi|fq|gr|hash|hypgeom|la|limb|lnf|mag|mantissa|mp_limb|mp_ptr|mp_size|mp_srcptr|mpf|mpn|mpoly|mpq|mpz|n|nf|nfloat|nmod|ordering|padic|partitions|perm|polynomial|prime|qadic|qfb|qnf|qqbar|qs|radix|relation|slong|thread|truth|ulong|unity|vector|z|zz).*",
    "^(__Bindgen|__builtin|__mpq|__mpz|__mpfr|__va|_IO|FILE|FLINT_FILE|pthread|size_t).*",
];

#[allow(dead_code)]
static BINDGEN_ALLOWLIST_VARS: &[&str] = &[
    "^_?(acb|acf|arb|arf|ca|dlog|fexpr|flint|fmpq|fmpz|fq|gr|mag|nmod|padic|qqbar|thread|truth)_.*",
    "^(ACB|ARB|ARF|BELL|BERNOULLI|CA|CRT|D_|DFT|DLOG|FEXPR|FLINT|FPWRAP|FQ|GR|LSYM|MAG|MAX|MPOLY|MUL|NF|NMOD|PADIC|QQBAR|SMALL|SQUARING|UWORD|WEAK|WORD)_.*",
];

// We will spawn a lot of shell commands with Command::new. The properway to
// handle errors (capturing the output and checking the exit code) is a bit
// verbose, so there is this macro.
macro_rules! cmd {
    ($cmd: expr) => {
        let mut cmd = $cmd;

        // Save the command string for error reporting
        let cmd_string = format!("{:?}", cmd);

        // Launch the command. Use the ? operator, so the macro only works in a
        // function that return a anyhow::Result. Typical errors (like
        // `./condigure` failing because of a missing dependency) are not
        // catched here, only IO errors.
        let exit = cmd
            .output()
            .context(format!("Command {} did not execute normally", cmd_string))?;

        // Check the exit code
        if !exit.status.success() {
            // Report error
            anyhow::bail!(
                "Command failed\nCommand: {}\n===== stdout\n{}===== stderr\n{}\n",
                cmd_string,
                String::from_utf8_lossy(&exit.stdout),
                String::from_utf8_lossy(&exit.stderr),
            )
        }
    };
}

// This contains a few relevant paths.
struct Conf {
    out_dir: PathBuf,           // OUT_DIR from Cargo
    bindgen_flint_rs: PathBuf,  // $OUT_DIR/flint.rs, generated by bindgen
    flint_include_dir: PathBuf, // $OUT_DIR/include, generated by `make install` from FLINT
    flint_lib_dir: PathBuf,     // $OUT_DIR/lib, idem
}

impl Conf {
    fn new() -> Self {
        // OUT_DIR is where cargo asks us to put the build artifacts
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap())
            .canonicalize()
            .unwrap();
        Conf {
            out_dir: out_dir.clone(),
            bindgen_flint_rs: out_dir.join("flint.rs"),
            flint_include_dir: out_dir.join("include"),
            flint_lib_dir: out_dir.join("lib"),
        }
    }

    // STEP 1.
    //
    // Compile FLINT and install it with prefix `OUT_DIR`, so it populates
    // `$OUT_DIR/include` and `$OUT_DIR/lib`.
    fn build_flint(&self) -> Result<()> {
        let flint_root_dir = self.out_dir.join("flint");

        // FLINT does not support out-of-tree compilation, so we copy the source
        let mut cp = Command::new("cp");
        cp.arg("-Rp") // keeps the timestamps, so it avoids to trigger unnecessary rebuild
            // .arg("--update") // copy only what we need
            .arg("flint")
            .arg(&self.out_dir);

        cmd! { cp } // lauch the command

        // We now follow the instructions in flint/INSTALL.md

        // Do not bootstrap if `configure` already exists
        if !flint_root_dir.join("configure").is_file() {
            let mut bootstrap = Command::new("sh");
            bootstrap.current_dir(&flint_root_dir).arg("./bootstrap.sh");

            cmd! { bootstrap }
        }

        // Do not configure if `Makefile` already exists. This is maybe a bit
        // optimistic, I guess that there are times where we want to refresh the
        // Makefile. But when? In most cases, we don't.
        if !flint_root_dir.join("Makefile").is_file() {
            let mut configure = Command::new("sh");

            configure
                .current_dir(&flint_root_dir)
                .arg("./configure")
                .arg("--prefix") // ask that the files are install in OUT_DIR, not `/usr`
                .arg(&self.out_dir)
                .arg("--disable-shared"); // it is not advised to generate dynamic libraries in crates

            // In case, we have the crate `gmp-mpfr-sys` available, we indicate
            // to the configuration scripts where to find the headers and the
            // lib files of GMP and MPFR.
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

            cmd! { configure }
        }

        // Compile, if `libflint.a` does not already exist
        if !flint_root_dir.join("libflint.a").is_file() {
            let mut make = Command::new("make");
            make.current_dir(&flint_root_dir);
            make.env("MAKEFLAGS", std::env::var("CARGO_MAKEFLAGS").unwrap());
            cmd! { make }
        }

        // Install in OUT_DIR
        let mut make_install = Command::new("make");
        make_install.current_dir(&flint_root_dir).arg("install");

        cmd! { make_install };

        // These are environment variables that will be accessible to crates
        // depending on flint3-sys. They will be named DEP_FLINT_LIB_DIR and
        // DEP_FLINT_INCLUDE_DIR. See the Cargo book for more details.
        println!("cargo::metadata=LIB_DIR={}", self.flint_lib_dir.display());
        println!(
            "cargo::metadata=INCLUDE_DIR={}",
            self.flint_include_dir.display()
        );

        Ok(())
    }
}

// STEP 2 (without `force-bindgen`)
//
// We copy the files in ./bindgen to the OUR_DIR.
#[cfg(not(feature = "force-bindgen"))]
impl Conf {
    fn bindgen(&self) -> Result<()> {
        println!(
            "cargo::rerun-if-changed={}",
            &self.bindgen_flint_rs.display()
        );
        let mut cp = Command::new("cp");
        cp.arg("-Rp") // the -p flag avoids trigerring build.rs for no reason
            .arg("./bindgen/flint.rs")
            .arg(&self.bindgen_flint_rs);
        cmd! { cp };
        Ok(())
    }
}

// STEP 2 (with `bindgen`)
//
// This is the tricky part.
#[cfg(feature = "force-bindgen")]
impl Conf {
    // Compute a list of installed FLINT headers, minus the selected skip set.
    fn flint_headers(&self, skip_headers: &[&str]) -> Result<Vec<PathBuf>> {
        use std::{collections::HashSet, ffi::OsStr};

        // When we arrive here, `make install` is completed, so the FLINT
        // headers are in $OUT_DIR/include/flint.
        let flint_header_dir = self.flint_include_dir.join("flint");

        // Sanity check: there must be a file $OUT_DIR/include/flint/flint.h
        anyhow::ensure!(
            flint_header_dir.join("flint.h").is_file(),
            "Cannot find `flint.h`"
        );

        // The header paths will be put here.
        let mut headers = Vec::new();

        // Construct a HashSet of headers to be skipped.
        let mut skip = HashSet::new();
        for file in skip_headers {
            skip.insert(OsStr::new(*file));
        }

        // Iterate over all the files in $OUT_DIR/include/flint
        let entries = flint_header_dir.read_dir()?;
        let header_extension = OsStr::new("h");
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension() != Some(header_extension) {
                // This is not a header (what can it be ?)
                continue;
            }
            if skip.contains(&path.file_name().unwrap()) {
                // This is in the skip set
                continue;
            }

            // This is a good header
            headers.push(path)
        }

        Ok(headers)
    }

    fn bindgen(&self) -> Result<()> {
        // Headers that should be emitted if declarations originate there.
        let allowlist_headers: Vec<_> = self.flint_headers(SKIP_BINDGEN_HEADERS)?;

        // Headers that should be passed as clang entry points. Type/template
        // headers are still available transitively through the public modules.
        let direct_headers: Vec<_> = self.flint_headers(SKIP_DIRECT_HEADERS)?;

        let mut builder = bindgen::Builder::default();

        // Add the headers that may contribute public bindings.
        for header in &allowlist_headers {
            // when dealing with the file system, every single function maybe fail...
            let h = header.to_str().context("Non unicode path")?;

            builder = builder.allowlist_file(h);
            // We are using bindgen in allowlisting mode, see
            // https://rust-lang.github.io/rust-bindgen/allowlisting.html
            // This avoids bringing all of GMP in the binding
        }

        // Add only real public entry headers as clang translation units.
        for header in &direct_headers {
            let h = header.to_str().context("Non unicode path")?;
            builder = builder.header(h);
        }

        for pattern in BINDGEN_ALLOWLIST_FUNCTIONS {
            builder = builder.allowlist_function(pattern);
        }

        for pattern in BINDGEN_ALLOWLIST_TYPES {
            builder = builder.allowlist_type(pattern);
        }

        for pattern in BINDGEN_ALLOWLIST_VARS {
            builder = builder.allowlist_var(pattern);
        }

        let bindings = builder
            // .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            // useful to echo some cargo:force, but disabled because it triggers
            // too many recompilations.
            .derive_default(false) // use mem::zeroed() instead, or MaybeUninit
            .derive_copy(false) // nothing (?) in FLINT is Copy
            .derive_debug(false) // useless
            .generate_cstr(true) // recommended by bindgen's doc
            .blocklist_function("__.*") // block internal items
            .blocklist_var("__.*") // block internal items
            .rust_target(bindgen::RustTarget::stable(82, 0).ok().unwrap())
            .rust_edition(bindgen::RustEdition::Edition2021)
            .layout_tests(false)
            .formatter(bindgen::Formatter::None)
            .generate()?;

        // After that, we have our bindings in OUT_DIR/flint.rs
        bindings.write_to_file(&self.bindgen_flint_rs)?;

        // The maintenaire of `flint3-sys` may use the environment variable
        // KEEP_BINDGEN_OUTPUT to save the result of bindgen and release it,
        // so that the use downstream do not have to run bindgen themselves.
        // See README.md
        println!("cargo::rerun-if-env-changed=KEEP_BINDGEN_OUTPUT");
        if std::env::var("KEEP_BINDGEN_OUTPUT").is_ok() {
            std::fs::copy(
                &self.bindgen_flint_rs,
                &std::path::Path::new("./bindgen/flint.rs"),
            )
            .context(format!(
                "Failed to copy `{}`",
                self.bindgen_flint_rs.display()
            ))?;
        }

        Ok(())
    }
}

// Out the three steps together
fn main() -> Result<()> {
    let conf = Conf::new();

    /////////////////
    // build FLINT //
    /////////////////

    conf.build_flint()?;

    // make sure that we have the correct files at the correct place
    anyhow::ensure!(
        conf.flint_include_dir.join("flint/flint.h").is_file(),
        "Compilation is successful, but `flint/flint.h` is not where it should"
    );

    // idem
    anyhow::ensure!(
        conf.flint_lib_dir.join("libflint.a").is_file(),
        "Compilation is successful, but `libflint.a` is not where it should"
    );

    // Instruct cargo that he has to link against libflint.a and its dependencies.
    // The order seems to be important with some linkers...
    println!("cargo::rustc-link-lib=flint");
    println!("cargo::rustc-link-lib=mpfr");
    println!("cargo::rustc-link-lib=gmp");
    println!(
        "cargo::rustc-link-search=native={}",
        conf.flint_lib_dir.display()
    );

    if cfg!(feature = "gmp-mpfr-sys") {
        // I am not completely sure that this is useful
        println!(
            "cargo::rustc-link-search=native={}",
            std::env::var("DEP_GMP_LIB_DIR")?
        );
    }

    ////////////////////////
    // binding generation //
    ////////////////////////

    // Unless the feature `force-bindgen` is unable, this simply takes the files
    // from the directory `./bindgen`.
    conf.bindgen()?;

    anyhow::ensure!(conf.bindgen_flint_rs.is_file(), "Cannot find `flint.rs`");

    Ok(())
}
