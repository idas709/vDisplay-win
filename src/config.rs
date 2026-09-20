use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub display_index: usize,
    pub target_fps: u32,
    pub zoom_sensitivity: f32,
}

impl Default for AppConfig {
    fn default() -> Self { Self { display_index: 0, target_fps: 60, zoom_sensitivity: 0.1 } }
}
