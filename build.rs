//! Windows: embeds the exe icon and version metadata (version from Cargo.toml) as resources.

fn main() {
    println!("cargo::rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    {
        // FileDescription is the name Task Manager shows for the process.
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico").set("ProductName", "dropdl").set("FileDescription", "dropdl");
        res.compile().expect("compile Windows resources");
    }
}
