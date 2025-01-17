extern crate core;

use cc::Build;
use std::path::Path;
use walkdir::WalkDir;

/// Finds all files with an extension, ignoring some.
///
/// TODO: If this gets more complicated, we might want to inline the list of files available in meson.
fn glob_import<P: AsRef<Path>>(root: P, extenstion: &str, exclude: &str) -> Vec<String> {
    WalkDir::new(root)
        .into_iter()
        .map(|x| x.unwrap())
        .filter(|x| x.path().to_str().unwrap().ends_with(extenstion))
        .map(|x| x.path().to_str().unwrap().to_string())
        .filter(|x| !x.contains(exclude))
        .collect()
}

/// Attempts to compile assembly units and links them to the current compilation build.
///
/// Ok, I tried to clean this up for 0.3 since I found the previous build logic a bit hard to follow. This works for me,
/// but there is a chance I broke things, apologies if that's why you're here.
///
/// Feel free to submit PRs improving this file, but try to keep the logic 'KISS' and minimize branches and nesting if possible.
#[allow(unused)]
fn try_compile_nasm(cc_build_command: &mut Build, root: &str) {
    // If NASM isn't found we don't use it
    // if Command::new("nasm").status().is_err() {
    //     println!("Command `nasm` not found, things will be slower.");
    //     return;
    // }

    let target = std::env::var("TARGET").unwrap();

    let is_64bits = target.starts_with("x86_64") || target.starts_with("aarch64");
    let is_x86 = target.starts_with("x86_64") || target.starts_with("i686");
    let is_arm = target.starts_with("arm") || target.starts_with("arm7") || target.starts_with("aarch64");
    let is_windows = target.contains("windows");
    let is_unix = target.contains("apple") || target.contains("linux");

    // `_UNUSED` is just a define that doesn't interfere with any actual define in OpenH264.
    let mut define_cpp = "_UNUSED";
    let mut define_asm = "_UNUSED";
    let mut define_prefix = "_UNUSED";
    let mut asm_dir = "";
    let mut extension = "";
    let mut exclusion = "";

    if is_x86 {
        extension = ".asm";
        define_cpp = "X86_ASM";
        asm_dir = "x86";
        exclusion = "asm_inc.asm";

        if is_windows && is_64bits {
            define_asm = "WIN64";
        } else if is_unix && is_64bits {
            define_asm = "UNIX64";
            define_prefix = "PREFIX";
        } else {
            define_asm = "X86_32";
        }
    } else if is_arm {
        extension = ".S";

        if is_64bits {
            define_cpp = "HAVE_NEON_AARCH64";
            asm_dir = "arm64";
            define_asm = "HAVE_NEON_AARCH64";
            exclusion = "arm_arch64_common_macro.S";
        } else {
            define_cpp = "HAVE_NEON";
            asm_dir = "arm";
            define_asm = "HAVE_NEON";
            exclusion = "arm_arch_common_macro.S";
        }
    }

    // Try to compile NASM targets
    let mut nasm_build = nasm_rs::Build::new();
    let mut nasm_build = nasm_build
        .include(format!("upstream/codec/common/{}/", asm_dir))
        .define(define_asm, Some(define_asm))
        .define(define_prefix, None);

    // Run `nasm` and store result.
    let Ok(object_files) = nasm_build.files(glob_import(root, extension, exclusion)).compile_objects() else {
        return;
    };

    // This here only _EXTENDS_ the build command we got passed, it doesn't
    // _RUN_ any build command on its own (we still invoked `nasm` above
    // though).
    cc_build_command.define(define_cpp, None);

    for object in &object_files {
        cc_build_command.object(object);
    }
}

/// Builds an OpenH264 sub-library and adds it to the project.
fn compile_and_add_openh264_static_lib(name: &str, root: &str, includes: &[&str]) {
    let mut cc_build = cc::Build::new();

    try_compile_nasm(&mut cc_build, root);

    cc_build
        .include("upstream/codec/api/wels/")
        .include("upstream/codec/common/inc/")
        .cpp(true)
        .warnings(false)
        .files(glob_import(root, ".cpp", "DllEntry.cpp")) // Otherwise fails when compiling on Linux
        .pic(true)
        // Upstream sets these two and if we don't we get segmentation faults on Linux and MacOS ... Happy times.
        .flag_if_supported("-fno-strict-aliasing")
        .flag_if_supported("-fstack-protector-all")
        .flag_if_supported("-fembed-bitcode")
        .flag_if_supported("-fno-common")
        .flag_if_supported("-undefined dynamic_lookup");

    for include in includes {
        cc_build.include(include);
    }

    cc_build.compile(format!("libopenh264_{}.a", name).as_str());

    println!("cargo:rustc-link-lib=static=openh264_{}", name);
}

fn main() {
    compile_and_add_openh264_static_lib("common", "upstream/codec/common", &[]);

    compile_and_add_openh264_static_lib(
        "processing",
        "upstream/codec/processing",
        &[
            "upstream/codec/processing/src/common/",
            "upstream/codec/processing/interface/",
        ],
    );

    #[cfg(feature = "decoder")]
    compile_and_add_openh264_static_lib(
        "decoder",
        "upstream/codec/decoder",
        &["upstream/codec/decoder/core/inc/", "upstream/codec/decoder/plus/inc/"],
    );

    #[cfg(feature = "encoder")]
    compile_and_add_openh264_static_lib(
        "encoder",
        "upstream/codec/encoder",
        &[
            "upstream/codec/encoder/core/inc/",
            "upstream/codec/encoder/plus/inc/",
            "upstream/codec/processing/interface/",
        ],
    );
}
