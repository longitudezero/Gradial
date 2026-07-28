fn main() {
    println!("cargo:rerun-if-changed=resources/gradial.rc");
    println!("cargo:rerun-if-changed=resources/gradial.exe.manifest");

    embed_resource::compile("resources/gradial.rc", embed_resource::NONE)
        .manifest_required()
        .expect("the Gradial Windows manifest must be embedded");
}
