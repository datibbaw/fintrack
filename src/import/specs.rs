use anyhow::{anyhow, Context, Result};
use rust_embed::RustEmbed;
use serde::de::DeserializeOwned;

#[derive(RustEmbed)]
#[folder = "specs/"]
struct Specs;

pub(crate) fn load<T: DeserializeOwned>(name: &str) -> Result<T> {
    let content = Specs::get(name)
        .ok_or_else(|| anyhow!("Spec {name} not found"))?
        .data;
    let result = serde_yaml::from_slice(&content)
        .with_context(|| format!("failed to parse spec {name}: invalid YAML or missing fields"))?;
    Ok(result)
}
