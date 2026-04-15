mod app;
mod config;
mod keys;
mod menu;
mod menubar;
mod switcher;
mod zoning;

fn main() {
    sola_app::run::<app::ShellApp>();
}
