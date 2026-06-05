use crate::bundler::constants::{VIRTUAL_ENV, VIRTUAL_I18N, VIRTUAL_SUGG};
use rolldown::plugin::{
    HookLoadArgs, HookLoadOutput, HookLoadReturn, HookResolveIdArgs, HookResolveIdOutput,
    HookResolveIdReturn, HookUsage, LoadPluginContext, Plugin, PluginContext,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use sugg_core::path_to_slash;

#[derive(Debug)]
pub struct VirtualPlugin {
    pub virtual_modules: HashMap<String, String>,
    pub env_code: String,
    pub i18n_modules: HashMap<String, String>,
}

impl Plugin for VirtualPlugin {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("virtual-plugin")
    }

    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load
    }

    async fn resolve_id(
        &self,
        _ctx: &PluginContext,
        args: &HookResolveIdArgs<'_>,
    ) -> HookResolveIdReturn {
        let specifier = path_to_slash(std::path::Path::new(args.specifier));
        if specifier == VIRTUAL_ENV
            || specifier == VIRTUAL_SUGG
            || specifier.starts_with(VIRTUAL_I18N)
            || self.virtual_modules.contains_key(&specifier)
        {
            return Ok(Some(HookResolveIdOutput {
                id: specifier.into(),
                ..Default::default()
            }));
        }
        Ok(None)
    }

    async fn load(&self, _ctx: Arc<LoadPluginContext>, args: &HookLoadArgs<'_>) -> HookLoadReturn {
        if args.id == VIRTUAL_ENV || args.id == VIRTUAL_SUGG {
            return Ok(Some(HookLoadOutput {
                code: self.env_code.clone().into(),
                ..Default::default()
            }));
        }

        let id_normalized = path_to_slash(std::path::Path::new(args.id));

        if id_normalized.starts_with(VIRTUAL_I18N) {
            let code = self
                .i18n_modules
                .get(&id_normalized)
                .cloned()
                .unwrap_or_default();
            return Ok(Some(HookLoadOutput {
                code: code.into(),
                ..Default::default()
            }));
        }

        if let Some(code) = self.virtual_modules.get(&id_normalized) {
            return Ok(Some(HookLoadOutput {
                code: code.clone().into(),
                ..Default::default()
            }));
        }
        Ok(None)
    }
}
