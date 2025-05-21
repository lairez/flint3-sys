use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::{anyhow, bail};
use bindgen::RustTarget;

static SKIP_HEADERS: &[&str] = &[
    "NTL-interface.h",
    "crt_helpers.h",
    "longlong_asm_clang.h",
    "longlong_asm_gcc.h",
    "longlong_div_gnu.h",
    "longlong_msc_arm64.h",
    "longlong_msc_x86.h",
    "mpfr_mat.h", // deprecated
    "mpfr_vec.h", // deprecated
    "gmpcompat.h",
    "fft_small.h", // seems to cause some issues, but not so
    // important, fft_small is still used by Flint even if the header
    // is not exposed.
    "machine_vectors.h", // idem
    "mpn_extras.h",
    "gettimeofday.h",
];

// Compute the list of all Flint headers (minus the one we skip)
fn flint_headers() -> Result<Vec<PathBuf>> {
    let lib = pkg_config::probe_library("flint")?;

    let mut flint_header_dir = None;
    for include_path in &lib.include_paths {
        let maybe_flint_header_dir = Path::new(include_path).join("flint");
        if maybe_flint_header_dir.join("flint.h").is_file() {
            flint_header_dir = Some(maybe_flint_header_dir);
            break;
        }
    }

    // This is most probably `/usr/include/flint`
    let flint_header_dir =
        flint_header_dir.ok_or(anyhow!("Cannot find the Flint header directory"))?;
    if !flint_header_dir.is_dir() {
        bail!("Cannot find the Flint header directory")
    }

    // Now, list all the headers in `/usr/include/flint/`
    let entries = flint_header_dir.read_dir()?;
    let mut headers = Vec::new();

    let mut skip = HashSet::new();
    for file in SKIP_HEADERS {
        skip.insert(OsStr::new(*file));
    }

    let header_extension = OsStr::new("h");
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension() != Some(header_extension) {
            continue;
        }
        if skip.contains(&path.file_name().unwrap()) {
            continue;
        }
        headers.push(path)
    }

    Ok(headers)
}

fn main() -> Result<()> {
    // Target directory of `cargo build`
    let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // All relevant Flint headers
    let headers: Vec<_> = flint_headers()?;

    // Target file for bindgen's --wrap-static
    // https://github.com/rust-lang/rust-bindgen/discussions/2405
    let extern_fp = out_path.join("extern.c");

    // Target file for bindgen
    let out_fp = out_path.join("flint.rs");

    /////////////
    // bindgen //
    /////////////

    let mut builder = bindgen::Builder::default();
    for header in headers {
        let h = header.to_str().ok_or(anyhow!("Non unicode path"))?;
        builder = builder.allowlist_file(h).header(h);
        // We are using bindgen in allowlisting mode, see
        // https://rust-lang.github.io/rust-bindgen/allowlisting.html
        // This avoids bringing all of GMP in the binding
    }
    let bindings = builder
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new())) // useful to echo some cargo:rerun
        .derive_default(true)                                      // useful to avoid too many MaybeUninit
        .derive_copy(false)                                        // nothing (?) in Flint is Copy
        .derive_debug(false)
        .wrap_static_fns(true) // deal with inline functions
        .wrap_static_fns_path(&extern_fp) // idem
        .generate_cstr(true) // recommended by bindgen's doc
        .merge_extern_blocks(true)
        .blocklist_function("__.*") // block internal items
        .blocklist_var("__.*") // block internal items
        .rust_target(RustTarget::stable(82, 0).ok().unwrap())
        // There are still issues with 2024 edition
        // https://github.com/rust-lang/rust-bindgen/issues/3180
        .rust_edition(bindgen::RustEdition::Edition2021)
        .formatter(bindgen::Formatter::Prettyplease)
        .generate()?;

    bindings.write_to_file(out_fp)?;

    /////////////////////////////
    // compilation of extern.c //
    /////////////////////////////

    cc::Build::new()
        .file(&extern_fp)
        .flags(["-lflint", "-lgmp", "-lmpfr"])
        .flags(["-Wno-old-style-declaration", "-Wno-unused-parameter", "-Wno-sign-compare"])
        .try_compile("extern")?;

    println!("cargo:rustc-link-lib=static=extern");

    Ok(())
}
