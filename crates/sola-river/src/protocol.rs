//! Client bindings for River's custom Wayland protocols, generated at
//! compile time from the XML files vendored in `crates/sola-river/protocols/`.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all
)]

pub mod river_window_management_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/river-window-management-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/river-window-management-v1.xml");
}

pub mod wlr_output_management_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        wayland_scanner::generate_interfaces!("protocols/wlr-output-management-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/wlr-output-management-unstable-v1.xml");
}

pub mod river_xkb_bindings_v1 {
    use wayland_client;

    // The bindings protocol references `river_seat_v1` from the WM protocol.
    use crate::protocol::river_window_management_v1::*;

    pub mod __interfaces {
        use crate::protocol::river_window_management_v1::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/river-xkb-bindings-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/river-xkb-bindings-v1.xml");
}

pub mod wlr_virtual_pointer_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/wlr-virtual-pointer-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/wlr-virtual-pointer-unstable-v1.xml");
}

pub mod ext_foreign_toplevel_list_v1 {
    use wayland_client;

    pub mod __interfaces {
        wayland_scanner::generate_interfaces!("protocols/ext-foreign-toplevel-list-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/ext-foreign-toplevel-list-v1.xml");
}

pub mod ext_image_capture_source_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    use crate::protocol::ext_foreign_toplevel_list_v1::*;

    pub mod __interfaces {
        use crate::protocol::ext_foreign_toplevel_list_v1::__interfaces::*;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/ext-image-capture-source-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/ext-image-capture-source-v1.xml");
}

pub mod ext_image_copy_capture_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    use crate::protocol::ext_image_capture_source_v1::*;

    pub mod __interfaces {
        use crate::protocol::ext_image_capture_source_v1::__interfaces::*;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/ext-image-copy-capture-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/ext-image-copy-capture-v1.xml");
}

pub mod wlr_screencopy_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/wlr-screencopy-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/wlr-screencopy-unstable-v1.xml");
}

pub mod virtual_keyboard_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/virtual-keyboard-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/virtual-keyboard-unstable-v1.xml");
}

pub mod river_input_management_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/river-input-management-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/river-input-management-v1.xml");
}

pub mod river_libinput_config_v1 {
    use wayland_client;

    // `river_libinput_device_v1.input_device` references this type.
    use crate::protocol::river_input_management_v1::*;

    pub mod __interfaces {
        use crate::protocol::river_input_management_v1::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/river-libinput-config-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/river-libinput-config-v1.xml");
}

pub mod river_xkb_config_v1 {
    use wayland_client;

    // `river_xkb_keyboard_v1.input_device` references this type.
    use crate::protocol::river_input_management_v1::*;

    pub mod __interfaces {
        use crate::protocol::river_input_management_v1::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/river-xkb-config-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/river-xkb-config-v1.xml");
}

/// Enables standard `wlr-layer-shell-unstable-v1` for non-WM clients.
/// Binding this global is what tells River "the WM supports layer shell";
/// without it, River closes every layer surface immediately.
///
/// Required infrastructure for sola-kvm (and any other layer-shell client).
pub mod river_layer_shell_v1 {
    use wayland_client;

    // `get_output` / `get_seat` reference types from the WM protocol.
    use crate::protocol::river_window_management_v1::*;

    pub mod __interfaces {
        use crate::protocol::river_window_management_v1::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/river-layer-shell-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/river-layer-shell-v1.xml");
}
