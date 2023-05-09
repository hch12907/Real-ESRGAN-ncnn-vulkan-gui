#[cfg(target_os = "windows")]
extern crate embed_resource;

fn main() {
    #[cfg(target_os = "windows")]
    embed_resource::compile("realesrgan-ncnn-vulkan-gui.exe.rc");
}
