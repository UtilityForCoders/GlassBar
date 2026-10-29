//! Incrusta el ícono (`app.rc` -> `assets/icon.ico`) y el manifiesto (`app.manifest`).
//! Usa embed-resource para compilar con el Windows SDK (rc.exe).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("no se pudo compilar app.rc (¿falta el Windows SDK?)");
}
