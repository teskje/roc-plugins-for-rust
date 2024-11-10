use std::fs;

use crate::plugin::Plugin;

mod plugin;
mod roc_host;

fn main() {
    roc_host::init();

    let dir = fs::read_dir("plugins").unwrap();
    for entry in dir {
        let entry = entry.unwrap();
        let plugin_path = entry.path();

        println!("loading plugin from {}", plugin_path.to_str().unwrap());
        let plugin = Plugin::load(plugin_path);

        println!("invoking plugin: {}", plugin.name());
        plugin.invoke();

        println!();
    }
}
