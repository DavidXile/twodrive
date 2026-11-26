//! Private JSON-lines IPC for the Flutter desktop application.
#[path = "../desktop.rs"]
mod desktop;
fn main() -> anyhow::Result<()> {
    desktop::run()
}
