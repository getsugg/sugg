pub mod constants;
pub mod plugin;
pub use constants::*;

use crate::bundler::plugin::VirtualPlugin;
use rolldown::{BundlerBuilder, BundlerOptions, InputItem};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn bundle_virtual(
    entry_id: &str,
    virtual_modules: HashMap<String, String>,
    env_code: String,
    i18n_modules: HashMap<String, String>,
) -> anyhow::Result<String> {
    let options = BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("main".to_string()),
            import: entry_id.to_string(),
        }]),
        ..Default::default()
    };
    let plugin = VirtualPlugin {
        virtual_modules,
        env_code,
        i18n_modules,
    };
    let mut bundler = BundlerBuilder::default()
        .with_options(options)
        .with_plugins(vec![Arc::new(plugin)])
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create Bundler: {}", e))?;

    let output = bundler.generate().await.map_err(|e| {
        let mut msg = String::from("Bundling failed:\n");
        for diagnostic in e.into_vec() {
            msg.push_str(&format!("- [{:?}] {}\n", diagnostic.kind(), diagnostic));
        }
        anyhow::anyhow!(msg)
    })?;

    output
        .assets
        .into_iter()
        .find_map(|asset| match asset {
            rolldown_common::Output::Chunk(chunk) => Some(chunk.code.clone()),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("No JS code chunk found"))
}
