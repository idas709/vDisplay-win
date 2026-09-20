use std::{env, fs::File, path::PathBuf};

use ico::{IconDir, IconDirEntry, IconImage, ResourceType};

fn main() {
    if cfg!(target_os = "windows") {
        println!("cargo:rerun-if-changed=assets/app-icon.png");

        let icon_path =
            PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set")).join("app-icon.ico");
        let source =
            image::open("assets/app-icon.png").expect("failed to open assets/app-icon.png");
        let icon_file = File::create(&icon_path).expect("failed to create generated icon");
        let mut icon_dir = IconDir::new(ResourceType::Icon);
        for size in [16, 24, 32, 48, 64, 128, 256] {
            let icon = source
                .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            let icon_image = IconImage::from_rgba_data(size, size, icon.into_raw());
            icon_dir
                .add_entry(IconDirEntry::encode(&icon_image).expect("failed to encode icon size"));
        }
        icon_dir
            .write(icon_file)
            .expect("failed to write generated icon");

        let mut resource = winresource::WindowsResource::new();
        resource.set("ProductName", "Virtual Display Workspace");
        resource.set("FileDescription", "Virtual display workspace viewer");
        resource.set_icon(icon_path.to_str().expect("icon path is not valid UTF-8"));
        resource
            .compile()
            .expect("failed to compile Windows resources");
    }
}
