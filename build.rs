//! build.rs — embute `assets/app.ico` como recurso `1 ICON` en el exe Windows.
//!
//! Tray lo carga en runtime con `LoadImageW(MAKEINTRESOURCE(1), IMAGE_ICON)`;
//! el mismo recurso es el que muestra Explorer para el exe.
//!
//! Herramientas: `windres` del toolchain mingw-w64. Se linkea el `.o`
//! directamente con `rustc-link-arg-bins`: en un archive estático ld
//! descartaría el miembro (no define símbolos que nadie referencie).
//!
//! En host no-Windows (cargo test de la lib) no hace nada — no exige mingw.

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    println!("cargo:rerun-if-changed=assets/app.ico");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("assets").join("app.ico");
    assert!(icon.exists(), "falta {}", icon.display());

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let rc = out.join("app_icon.rc");
    let obj = out.join("app_icon.o");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", icon.display())).expect("escribir .rc");

    let windres = tool(&["x86_64-w64-mingw32-windres", "windres"], "windres");
    run(
        &windres,
        &[
            "-O",
            "coff",
            "-o",
            obj.to_str().unwrap(),
            rc.to_str().unwrap(),
        ],
    );

    // El .o entra tal cual (siempre incluido, no como archive).
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}

/// Prueba candidatos en PATH y devuelve el primero que responda a --version.
fn tool(candidates: &[&str], hint: &str) -> String {
    for name in candidates {
        let ok = Command::new(name)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return name.to_string();
        }
    }
    panic!("No encontre {hint} (binutils mingw-w64) en PATH; necesario para embutir el icono");
}

fn run(cmd: &str, args: &[&str]) {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("fallo ejecutar {cmd}: {e}"));
    assert!(status.success(), "{cmd} {args:?} termino con error");
}
