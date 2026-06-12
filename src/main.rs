mod app;
mod draw;
mod gui;
mod physics;
mod sim;
mod tree;
mod typed_idx;
mod utils;
use app::App;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "profile")]
    let _tracy = tracy_client::Client::start();
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}
