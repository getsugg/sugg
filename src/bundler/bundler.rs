use crate::bundler::plugin::VirtualPlugin;
use rolldown::{BundlerBuilder, BundlerOptions, InputItem};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn bundle_virtual(
    entry_id: &str,
    virtual_modules: HashMap<String, String>,
    env_code: String,
    i18n_modules: HashMap<String, String>,
) -> String {
    let options = BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("main".to_string()),
            import: entry_id.to_string(),
        }]),
        ..Default::default()
    };
    let plugin = VirtualPlugin { virtual_modules, env_code, i18n_modules };
    let mut bundler = BundlerBuilder::default()
        .with_options(options)
        .with_plugins(vec![Arc::new(plugin)])
        .build()
        .expect("Failed to create Bundler instance");

    let output = bundler.generate().await.expect("Rolldown bundling failed");
    output
        .assets
        .into_iter()
        .find_map(|asset| match asset {
            rolldown_common::Output::Chunk(chunk) => Some(chunk.code.clone()),
            _ => None,
        })
        .expect("Expected JS code chunk in output, but none found")
}
