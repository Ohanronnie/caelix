#![cfg(feature = "actix")]

#[caelix::main]
async fn runtime_entrypoint() -> std::io::Result<()> {
    Ok(())
}

#[test]
fn actix_feature_reexports_runtime_macro() {
    runtime_entrypoint().unwrap();
}
