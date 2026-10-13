#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    Normal,
    Loading,
    Empty,
    Error,
}
#[derive(Clone, PartialEq, Eq)]
pub struct HiddenApp {
    pub key: String,
    pub name: String,
    pub hidden_at: u64,
}

pub struct App {
    pub key: String,
    pub name: String,
    pub icon_path: String,
    pub volume: u8,
    pub peak: f32,
    pub muted: bool,
    pub icon: usize,
    pub route: Option<usize>,
    pub preferred_route: Option<String>,
}
pub struct Device {
    pub id: String,
    pub name: String,
    pub volume: u8,
    pub peak: f32,
    pub muted: bool,
    pub collapsed: bool,
    pub apps: Vec<App>,
}
pub struct Model {
    pub hidden_apps: Vec<HiddenApp>,
    pub devices: Vec<Device>,
    pub default: Option<usize>,
    pub scenario: Scenario,
    pub startup: bool,
    pub routing_available: bool,
    pub error: Option<String>,
}
impl Model {
    pub fn new() -> Self {
        Self {
            hidden_apps: vec![],
            devices: vec![],
            default: None,
            scenario: Scenario::Loading,
            startup: false,
            routing_available: false,
            error: None,
        }
    }
    pub fn set_volume(&mut self, device: usize, application: Option<usize>, value: u8) {
        let Some(d) = self.devices.get_mut(device) else {
            return;
        };
        if let Some(a) = application {
            if let Some(app) = d.apps.get_mut(a) {
                app.volume = value.min(100);
                app.muted = false;
            }
        } else {
            d.volume = value.min(100);
            d.muted = false;

        }
    }
}
