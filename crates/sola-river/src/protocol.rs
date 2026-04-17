//! Client bindings for River's custom Wayland protocols, generated at
//! compile time from the XML files vendored in `crates/sola-river/protocols/`.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, clippy::all)]

pub mod river_window_management_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "protocols/river-window-management-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "protocols/river-window-management-v1.xml"
    );
}

pub mod wlr_output_management_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        wayland_scanner::generate_interfaces!(
            "protocols/wlr-output-management-unstable-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "protocols/wlr-output-management-unstable-v1.xml"
    );
}

pub mod river_xkb_bindings_v1 {
    use wayland_client;

    // The bindings protocol references `river_seat_v1` from the WM protocol.
    use crate::protocol::river_window_management_v1::*;

    pub mod __interfaces {
        use crate::protocol::river_window_management_v1::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "protocols/river-xkb-bindings-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "protocols/river-xkb-bindings-v1.xml"
    );
}

pub mod virtual_keyboard_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "protocols/virtual-keyboard-unstable-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "protocols/virtual-keyboard-unstable-v1.xml"
    );
}
