mod app;
mod config;
mod keys;
mod launcher;
mod menu;
mod menubar;
mod session;
mod switcher;
mod zoning;

fn main() {
    sola_app::run::<app::ShellApp>();
}
