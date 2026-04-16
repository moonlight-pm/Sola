mod app;
mod applications;
mod config;
mod keys;
mod launcher;
mod menu;
mod menubar;
mod switcher;
mod zoning;

fn main() {
    sola_app::run::<app::ShellApp>();
}
