use std::error::Error;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use serde::{Deserialize, Serialize};

const DEFAULT_SERVER_ORIGIN: &str = "http://127.0.0.1:8080";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    name: String,
    model: String,
    serial: String,
    size: String,
    #[serde(rename = "type")]
    device_type: String,
    is_mounted: bool,
    is_frozen: bool,
    #[serde(rename = "isOSDrive")]
    is_os_drive: bool,
    #[serde(default)]
    active_dependencies: Vec<BlockDependency>,
}

#[derive(Clone, Debug, Deserialize)]
struct BlockDependency {
    name: String,
    #[serde(rename = "type")]
    dependency_type: String,
}

#[derive(Debug, Default, Deserialize)]
struct DeviceInventory {
    #[serde(default)]
    storage: Option<Vec<Device>>,
}

#[derive(Clone, Debug, Deserialize)]
struct WipeMethod {
    id: String,
    name: String,
    description: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WipePlan {
    decision: String,
    device_path: String,
    method: String,
    checks: Vec<PreflightCheck>,
}

#[derive(Clone, Debug, Deserialize)]
struct PreflightCheck {
    code: String,
    status: String,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct PreflightRequest<'a> {
    device_path: &'a str,
    method: &'a str,
    device_serial: &'a str,
    device_type: &'a str,
    device_model: &'a str,
}

struct ApiClient {
    origin: String,
    client: reqwest::Client,
}

impl ApiClient {
    fn new() -> Result<Self, reqwest::Error> {
        let origin = std::env::var("DZAP_SERVER_ORIGIN")
            .unwrap_or_else(|_| DEFAULT_SERVER_ORIGIN.to_string());
        Self::with_origin(origin)
    }

    fn with_origin(origin: impl Into<String>) -> Result<Self, reqwest::Error> {
        let origin = origin.into().trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self { origin, client })
    }

    async fn devices(&self) -> Result<Vec<Device>, String> {
        let inventory: DeviceInventory = self.get("/api/drives").await?;
        Ok(inventory.storage.unwrap_or_default())
    }

    async fn methods(&self, device_path: &str) -> Result<Vec<WipeMethod>, String> {
        let identifier = device_path.strip_prefix("/dev/").unwrap_or(device_path);
        self.get(&format!("/api/drive/{identifier}/wipe-methods"))
            .await
    }

    async fn preflight(&self, device: &Device, method: &WipeMethod) -> Result<WipePlan, String> {
        let request = PreflightRequest {
            device_path: &device.name,
            method: &method.id,
            device_serial: &device.serial,
            device_type: &device.device_type,
            device_model: &device.model,
        };
        let response = self
            .client
            .post(format!("{}/api/wipe/preflight", self.origin))
            .json(&request)
            .send()
            .await
            .map_err(|error| format!("preflight request failed: {error}"))?;
        decode_response(response, "preflight").await
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, String> {
        let response = self
            .client
            .get(format!("{}{path}", self.origin))
            .send()
            .await
            .map_err(|error| format!("backend request failed: {error}"))?;
        decode_response(response, "backend request").await
    }
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    operation: &str,
) -> Result<T, String> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("{operation} response could not be read: {error}"))?;
    if !status.is_success() {
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("error")?.as_str().map(str::to_string))
            .unwrap_or(body);
        return Err(format!("{operation} failed ({status}): {message}"));
    }
    serde_json::from_str(&body)
        .map_err(|error| format!("{operation} returned invalid data: {error}"))
}

struct App {
    api: ApiClient,
    devices: Vec<Device>,
    selected_device: usize,
    methods: Vec<WipeMethod>,
    selected_method: usize,
    plan: Option<WipePlan>,
    status: String,
}

impl App {
    fn new(api: ApiClient) -> Self {
        Self {
            api,
            devices: Vec::new(),
            selected_device: 0,
            methods: Vec::new(),
            selected_method: 0,
            plan: None,
            status: "Connecting to the local DZap backend...".to_string(),
        }
    }

    fn device(&self) -> Option<&Device> {
        self.devices.get(self.selected_device)
    }

    fn method(&self) -> Option<&WipeMethod> {
        self.methods.get(self.selected_method)
    }

    async fn refresh(&mut self) {
        match self.api.devices().await {
            Ok(devices) => {
                self.devices = devices;
                self.selected_device = self
                    .selected_device
                    .min(self.devices.len().saturating_sub(1));
                self.status = format!("Detected {} storage device(s).", self.devices.len());
                self.refresh_methods().await;
            }
            Err(error) => {
                self.devices.clear();
                self.methods.clear();
                self.plan = None;
                self.status = error;
            }
        }
    }

    async fn refresh_methods(&mut self) {
        self.plan = None;
        let Some(device_path) = self.device().map(|device| device.name.clone()) else {
            self.methods.clear();
            self.selected_method = 0;
            return;
        };
        match self.api.methods(&device_path).await {
            Ok(methods) => {
                self.methods = methods;
                self.selected_method = 0;
                if self.methods.is_empty() {
                    self.status = format!("No supported wipe method for {device_path}.");
                }
            }
            Err(error) => {
                self.methods.clear();
                self.selected_method = 0;
                self.status = error;
            }
        }
    }

    async fn move_device(&mut self, direction: i32) {
        let next = shifted_index(self.selected_device, self.devices.len(), direction);
        if next != self.selected_device {
            self.selected_device = next;
            self.refresh_methods().await;
        }
    }

    fn move_method(&mut self, direction: i32) {
        self.selected_method = shifted_index(self.selected_method, self.methods.len(), direction);
        self.plan = None;
    }

    async fn preflight(&mut self) {
        let Some(device) = self.device().cloned() else {
            self.status = "Select a detected storage device first.".to_string();
            return;
        };
        let Some(method) = self.method().cloned() else {
            self.status = "This device has no supported wipe method.".to_string();
            return;
        };
        self.status = format!("Running read-only preflight for {}...", device.name);
        match self.api.preflight(&device, &method).await {
            Ok(plan) => {
                self.status = format!("Preflight decision: {}.", plan.decision);
                self.plan = Some(plan);
            }
            Err(error) => {
                self.status = error;
                self.plan = None;
            }
        }
    }
}

fn shifted_index(current: usize, len: usize, direction: i32) -> usize {
    if len == 0 {
        return 0;
    }
    if direction < 0 {
        current.saturating_sub(1)
    } else {
        current.saturating_add(1).min(len - 1)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let api = ApiClient::new()?;
    let mut app = App::new(api);
    let mut terminal = ratatui::try_init()?;
    let result = run(&mut terminal, &mut app).await;
    ratatui::try_restore()?;
    result.map_err(Into::into)
}

async fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    terminal.draw(|frame| render(frame, app))?;
    app.refresh().await;

    loop {
        terminal.draw(|frame| render(frame, app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('r') => app.refresh().await,
            KeyCode::Up | KeyCode::Char('k') => app.move_device(-1).await,
            KeyCode::Down | KeyCode::Char('j') => app.move_device(1).await,
            KeyCode::Left | KeyCode::Char('h') => app.move_method(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('m') => app.move_method(1),
            KeyCode::Char('p') | KeyCode::Enter => app.preflight().await,
            _ => {}
        }
    }
}

fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width < 72 || area.height < 22 {
        frame.render_widget(
            Paragraph::new("DZap TUI needs a terminal of at least 72x22. Resize the window or switch to a larger console.")
                .wrap(Wrap { trim: true })
                .block(Block::bordered().title(" DZap TUI ")),
            area,
        );
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(12),
        Constraint::Length(3),
    ])
    .split(area);
    let columns =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(rows[1]);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " DZap TUI ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  read-only interface prototype"),
        ]))
        .block(Block::new().borders(Borders::BOTTOM)),
        rows[0],
    );

    render_devices(frame, app, columns[0]);
    render_details(frame, app, columns[1]);

    let status_style = if app.status.contains("failed") || app.status.contains("invalid") {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Gray)
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(" Up/Down device | Left/Right method | Enter preflight | R refresh | Q quit"),
            Line::styled(format!(" {}", app.status), status_style),
        ])
        .block(Block::new().borders(Borders::TOP)),
        rows[2],
    );
}

fn render_devices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items = if app.devices.is_empty() {
        vec![ListItem::new("No storage devices detected")]
    } else {
        app.devices
            .iter()
            .map(|device| {
                let mut flags = Vec::new();
                if device.is_os_drive {
                    flags.push("OS");
                }
                if device.is_mounted {
                    flags.push("mounted");
                }
                let suffix = if flags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", flags.join(", "))
                };
                ListItem::new(vec![
                    Line::styled(
                        format!("{}{}", device.name, suffix),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Line::from(format!("{} | {}", device.model, format_size(&device.size))),
                ])
            })
            .collect()
    };
    let list = List::new(items)
        .block(Block::bordered().title(" Storage devices "))
        .highlight_symbol("> ")
        .highlight_style(Style::default().fg(Color::Cyan));
    let mut state = ListState::default();
    if !app.devices.is_empty() {
        state.select(Some(app.selected_device));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_details(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let sections = Layout::vertical([
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Min(5),
    ])
    .split(area);

    let device_lines = match app.device() {
        Some(device) => {
            let dependencies = if device.active_dependencies.is_empty() {
                "none".to_string()
            } else {
                device
                    .active_dependencies
                    .iter()
                    .map(|dependency| {
                        format!("{} ({})", dependency.name, dependency.dependency_type)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            vec![
                labeled("Path", &device.name),
                labeled("Model", &device.model),
                labeled("Serial", &device.serial),
                labeled("Type", &device.device_type),
                labeled("Capacity", format_size(&device.size)),
                labeled(
                    "State",
                    format!(
                        "mounted={} frozen={} OS={} dependencies={dependencies}",
                        device.is_mounted, device.is_frozen, device.is_os_drive
                    ),
                ),
            ]
        }
        None => vec![Line::from("Refresh after the backend detects storage.")],
    };
    frame.render_widget(
        Paragraph::new(device_lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Selected device ")),
        sections[0],
    );

    let method_lines = match app.method() {
        Some(method) => vec![
            Line::styled(
                format!(
                    "{} of {}: {}",
                    app.selected_method + 1,
                    app.methods.len(),
                    method.name
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(method.description.clone()),
            Line::from(format!("Method ID: {}", method.id)),
        ],
        None => vec![Line::from(
            "No supported method is available for this device.",
        )],
    };
    frame.render_widget(
        Paragraph::new(method_lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Wipe method (←/→) ")),
        sections[1],
    );

    let (title, check_lines) = match &app.plan {
        Some(plan) => {
            let color = if plan.decision == "ready" {
                Color::Green
            } else {
                Color::Red
            };
            let mut lines = vec![Line::styled(
                format!(
                    "{} | {} | {}",
                    plan.decision.to_uppercase(),
                    plan.device_path,
                    plan.method
                ),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )];
            lines.extend(plan.checks.iter().map(|check| {
                let (marker, color) = if check.status == "passed" {
                    ("PASS", Color::Green)
                } else {
                    ("FAIL", Color::Red)
                };
                Line::from(vec![
                    Span::styled(
                        format!("{marker} {}: ", check.code),
                        Style::default().fg(color),
                    ),
                    Span::raw(check.message.clone()),
                ])
            }));
            (" Preflight result ", lines)
        }
        None => (
            " Preflight result ",
            vec![Line::from(
                "Press Enter to run the backend's read-only safety checks.",
            )],
        ),
    };
    frame.render_widget(
        Paragraph::new(check_lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
        sections[2],
    );
}

fn labeled(label: &str, value: impl ToString) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::Gray)),
        Span::raw(value.to_string()),
    ])
}

fn format_size(bytes: &str) -> String {
    let Ok(mut value) = bytes.parse::<f64>() else {
        return bytes.to_string();
    };
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use serde_json::json;

    #[test]
    fn selection_stays_inside_available_items() {
        assert_eq!(shifted_index(0, 0, 1), 0);
        assert_eq!(shifted_index(0, 3, -1), 0);
        assert_eq!(shifted_index(0, 3, 1), 1);
        assert_eq!(shifted_index(2, 3, 1), 2);
    }

    #[test]
    fn byte_sizes_are_human_readable() {
        assert_eq!(format_size("512"), "512 B");
        assert_eq!(format_size("1073741824"), "1.0 GiB");
        assert_eq!(format_size("unknown"), "unknown");
    }

    #[test]
    fn full_layout_renders_core_controls() {
        let api = ApiClient::new().unwrap();
        let mut app = App::new(api);
        app.devices.push(Device {
            name: "/dev/sdb".to_string(),
            model: "Test USB".to_string(),
            serial: "SERIAL".to_string(),
            size: "1073741824".to_string(),
            device_type: "USB Drive".to_string(),
            is_mounted: false,
            is_frozen: false,
            is_os_drive: false,
            active_dependencies: Vec::new(),
        });
        app.methods.push(WipeMethod {
            id: "overwrite_2_pass".to_string(),
            name: "2-Pass Complement Overwrite".to_string(),
            description: "Writes a pattern and its complement.".to_string(),
        });

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let screen = terminal.backend().to_string();

        assert!(screen.contains("DZap TUI"));
        assert!(screen.contains("/dev/sdb"));
        assert!(screen.contains("2-Pass Complement Overwrite"));
        assert!(screen.contains("Enter preflight"));
    }

    #[test]
    fn data_types_match_the_backend_contract() {
        let inventory: DeviceInventory = serde_json::from_value(json!({
            "storage": [{
                "name": "/dev/sdb",
                "model": "Test USB",
                "serial": "SERIAL",
                "size": "1073741824",
                "type": "USB Drive",
                "isMounted": false,
                "isFrozen": false,
                "isOSDrive": false,
                "activeDependencies": []
            }],
            "mobile": []
        }))
        .unwrap();
        let device = &inventory.storage.unwrap()[0];
        let request = serde_json::to_value(PreflightRequest {
            device_path: &device.name,
            method: "overwrite_2_pass",
            device_serial: &device.serial,
            device_type: &device.device_type,
            device_model: &device.model,
        })
        .unwrap();
        let plan: WipePlan = serde_json::from_value(json!({
            "decision": "ready",
            "devicePath": "/dev/sdb",
            "method": "overwrite_2_pass",
            "checks": [{
                "code": "device_exists",
                "status": "passed",
                "message": "Device is present."
            }]
        }))
        .unwrap();

        assert_eq!(request["DevicePath"], "/dev/sdb");
        assert_eq!(request["DeviceSerial"], "SERIAL");
        assert_eq!(plan.decision, "ready");
        assert_eq!(plan.checks[0].status, "passed");
    }
}
