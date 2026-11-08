use std::{env, fs, path::{Path, PathBuf}};

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read license directory") {
        let path = entry.expect("read license entry").path();
        if path.is_dir() { collect_files(&path, files); }
        else if path.is_file() { files.push(path); }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=assets/app/app.rc");
    println!("cargo:rerun-if-changed=assets/app/icon.ico");
    embed_resource::compile("assets/app/app.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();

    // Keep every third-party notice inside the distributed executable.
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed=licenses");
    println!("cargo:rerun-if-changed=THIRD_PARTY_NOTICES.md");
    let mut files = vec![root.join("THIRD_PARTY_NOTICES.md")];
    collect_files(&root.join("licenses"), &mut files);
    files.sort();
    let mut source = String::from("const LICENSE_FILES: &[(&str, &[u8])] = &[\n");
    for path in files {
        let name = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        source.push_str(&format!("({name:?}, include_bytes!({:?})),\n", path.to_string_lossy()));
    }
    source.push_str("];\n");
    fs::write(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("licenses.rs"), source)
        .expect("embed third-party licenses");
}
