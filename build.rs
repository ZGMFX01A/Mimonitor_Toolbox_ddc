fn main() {
    println!("cargo:rerun-if-changed=assets/app/icon.ico");
    embed_resource::compile("assets/app/app.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
