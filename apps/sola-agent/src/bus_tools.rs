use async_trait::async_trait;
use serde_json::{json, Value};
use sola_bus::BusClient;
use sola_bus::topics::Topic;
use std::sync::{Arc, Mutex};

pub struct RaiseAppTool {
    bus: Arc<Mutex<BusClient>>,
}

impl RaiseAppTool {
    pub fn new(bus: Arc<Mutex<BusClient>>) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl claurst_tools::Tool for RaiseAppTool {
    fn name(&self) -> &str { "raise_app" }
    fn description(&self) -> &str { "Bring a running Sola app to the foreground by its app_id" }
    fn permission_level(&self) -> claurst_tools::PermissionLevel { claurst_tools::PermissionLevel::None }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "app_id": { "type": "string", "description": "The app_id to raise" } },
            "required": ["app_id"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &claurst_tools::ToolContext) -> claurst_tools::ToolResult {
        let app_id = input["app_id"].as_str().unwrap_or("").to_string();
        if app_id.is_empty() {
            return claurst_tools::ToolResult { content: "app_id is required".into(), is_error: true, metadata: None };
        }
        match self.bus.lock() {
            Ok(mut client) => {
                let _ = client.emit(Topic::RaiseApp(app_id.clone()));
                claurst_tools::ToolResult { content: format!("Raised app: {}", app_id), is_error: false, metadata: None }
            }
            Err(e) => claurst_tools::ToolResult { content: format!("Bus error: {}", e), is_error: true, metadata: None },
        }
    }
}

pub struct LaunchAppTool {
    bus: Arc<Mutex<BusClient>>,
}

impl LaunchAppTool {
    pub fn new(bus: Arc<Mutex<BusClient>>) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl claurst_tools::Tool for LaunchAppTool {
    fn name(&self) -> &str { "launch_app" }
    fn description(&self) -> &str { "Launch a Sola app by its app_id" }
    fn permission_level(&self) -> claurst_tools::PermissionLevel { claurst_tools::PermissionLevel::None }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "app_id": { "type": "string", "description": "The app_id to launch" } },
            "required": ["app_id"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &claurst_tools::ToolContext) -> claurst_tools::ToolResult {
        let app_id = input["app_id"].as_str().unwrap_or("").to_string();
        if app_id.is_empty() {
            return claurst_tools::ToolResult { content: "app_id is required".into(), is_error: true, metadata: None };
        }
        match self.bus.lock() {
            Ok(mut client) => {
                let _ = client.emit(Topic::LaunchApp(app_id.clone()));
                claurst_tools::ToolResult { content: format!("Launched app: {}", app_id), is_error: false, metadata: None }
            }
            Err(e) => claurst_tools::ToolResult { content: format!("Bus error: {}", e), is_error: true, metadata: None },
        }
    }
}

pub struct ListAppsTool {
    bus: Arc<Mutex<BusClient>>,
}

impl ListAppsTool {
    pub fn new(bus: Arc<Mutex<BusClient>>) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl claurst_tools::Tool for ListAppsTool {
    fn name(&self) -> &str { "list_apps" }
    fn description(&self) -> &str { "List all running Sola apps with their app_id, name, and window count" }
    fn permission_level(&self) -> claurst_tools::PermissionLevel { claurst_tools::PermissionLevel::None }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _input: Value, _ctx: &claurst_tools::ToolContext) -> claurst_tools::ToolResult {
        match self.bus.lock() {
            Ok(mut client) => {
                let _ = client.emit(Topic::ListApps);
                // Poll for Apps response (up to 500ms)
                for _ in 0..50 {
                    if let Some(msg) = client.try_recv() {
                        if let Some(Topic::Apps(apps)) = Topic::parse(&msg) {
                            let list: Vec<String> = apps
                                .iter()
                                .map(|a| format!("{}: {} ({} windows)", a.app_id, a.name, a.window_count))
                                .collect();
                            return claurst_tools::ToolResult {
                                content: list.join("\n"),
                                is_error: false,
                                metadata: None,
                            };
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                claurst_tools::ToolResult { content: "Timeout waiting for app list".into(), is_error: true, metadata: None }
            }
            Err(e) => claurst_tools::ToolResult { content: format!("Bus error: {}", e), is_error: true, metadata: None },
        }
    }
}

pub fn create_bus_tools(bus: Arc<Mutex<BusClient>>) -> Vec<Box<dyn claurst_tools::Tool>> {
    vec![
        Box::new(RaiseAppTool::new(bus.clone())),
        Box::new(LaunchAppTool::new(bus.clone())),
        Box::new(ListAppsTool::new(bus)),
    ]
}
