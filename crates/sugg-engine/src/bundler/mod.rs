pub mod codegen;
pub mod constants;
pub mod plugin;
pub use constants::*;

use crate::bundler::plugin::VirtualPlugin;
use rolldown::{BundlerBuilder, BundlerOptions, InjectImport, InputItem};
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
        inject: Some(vec![
            InjectImport::named("createCompletion".into(), None, "virtual:env".into()),
            InjectImport::named("readJson".into(), None, "virtual:env".into()),
            InjectImport::named("cache".into(), None, "virtual:env".into()),
        ]),
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
        let msg = e
            .into_vec()
            .into_iter()
            .map(|d| {
                d.to_diagnostic()
                    .convert_to_string(false)
                    .replace("__v_stat_", "")
                    .replace("__v_dyn_", "")
            })
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::anyhow!("Bundling failed:\n{msg}")
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
