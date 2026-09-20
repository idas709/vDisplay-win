use anyhow::Result;
use winit::window::Icon;

pub fn load() -> Result<Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/app-icon.png"))?.to_rgba8();
    let (width, height) = image.dimensions();
    Ok(Icon::from_rgba(image.into_raw(), width, height)?)
}
