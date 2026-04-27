mod app;
mod keys;
mod launcher;
mod menu;
mod menubar;
mod switcher;
mod zoning;

fn main() {
    sola_app::run::<app::ShellApp>();
}
