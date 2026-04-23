use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sola_app::config::JsonConfig;
use sola_bus::topics::Zone;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShellConfig {
    #[serde(default)]
    pub zones: HashMap<String, Zone>,
}

impl JsonConfig for ShellConfig {
    const FILE_NAME: &'static str = "shell.json";
}
