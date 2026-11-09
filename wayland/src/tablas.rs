// Generado por tools/wayland_tablas.py desde wayland/protocolos/*.xml (licencia MIT, ver esos ficheros).
// No se edita a mano.

use crate::{Arg, Interface, Kind, Message};

pub static WL_DISPLAY: Interface = Interface {
    name: "wl_display",
    version: 1,
    requests: &[
        Message { name: "sync", since: 1, destructor: false, args: &[Arg { name: "callback", kind: Kind::NewId, nullable: false, interface: Some("wl_callback") }] },
        Message { name: "get_registry", since: 1, destructor: false, args: &[Arg { name: "registry", kind: Kind::NewId, nullable: false, interface: Some("wl_registry") }] },
    ],
    events: &[
        Message { name: "error", since: 1, destructor: false, args: &[Arg { name: "object_id", kind: Kind::Object, nullable: false, interface: None }, Arg { name: "code", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "message", kind: Kind::Str, nullable: false, interface: None }] },
        Message { name: "delete_id", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_display`.
pub mod wl_display {
    pub mod req {
        pub const SYNC: u16 = 0;
        pub const GET_REGISTRY: u16 = 1;
    }
    pub mod ev {
        pub const ERROR: u16 = 0;
        pub const DELETE_ID: u16 = 1;
    }
}

pub static WL_REGISTRY: Interface = Interface {
    name: "wl_registry",
    version: 1,
    requests: &[
        Message { name: "bind", since: 1, destructor: false, args: &[Arg { name: "name", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "id", kind: Kind::NewId, nullable: false, interface: None }] },
    ],
    events: &[
        Message { name: "global", since: 1, destructor: false, args: &[Arg { name: "name", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "interface", kind: Kind::Str, nullable: false, interface: None }, Arg { name: "version", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "global_remove", since: 1, destructor: false, args: &[Arg { name: "name", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_registry`.
pub mod wl_registry {
    pub mod req {
        pub const BIND: u16 = 0;
    }
    pub mod ev {
        pub const GLOBAL: u16 = 0;
        pub const GLOBAL_REMOVE: u16 = 1;
    }
}

pub static WL_CALLBACK: Interface = Interface {
    name: "wl_callback",
    version: 1,
    requests: &[

    ],
    events: &[
        Message { name: "done", since: 1, destructor: true, args: &[Arg { name: "callback_data", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_callback`.
pub mod wl_callback {
    pub mod req {
    }
    pub mod ev {
        pub const DONE: u16 = 0;
    }
}

pub static WL_COMPOSITOR: Interface = Interface {
    name: "wl_compositor",
    version: 7,
    requests: &[
        Message { name: "create_surface", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_surface") }] },
        Message { name: "create_region", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_region") }] },
        Message { name: "release", since: 7, destructor: true, args: &[] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `wl_compositor`.
pub mod wl_compositor {
    pub mod req {
        pub const CREATE_SURFACE: u16 = 0;
        pub const CREATE_REGION: u16 = 1;
        pub const RELEASE: u16 = 2;
    }
    pub mod ev {
    }
}

pub static WL_SHM_POOL: Interface = Interface {
    name: "wl_shm_pool",
    version: 3,
    requests: &[
        Message { name: "create_buffer", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_buffer") }, Arg { name: "offset", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "stride", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "format", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "resize", since: 1, destructor: false, args: &[Arg { name: "size", kind: Kind::Int, nullable: false, interface: None }] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `wl_shm_pool`.
pub mod wl_shm_pool {
    pub mod req {
        pub const CREATE_BUFFER: u16 = 0;
        pub const DESTROY: u16 = 1;
        pub const RESIZE: u16 = 2;
    }
    pub mod ev {
    }
}

pub static WL_SHM: Interface = Interface {
    name: "wl_shm",
    version: 3,
    requests: &[
        Message { name: "create_pool", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_shm_pool") }, Arg { name: "fd", kind: Kind::Fd, nullable: false, interface: None }, Arg { name: "size", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "release", since: 2, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "format", since: 1, destructor: false, args: &[Arg { name: "format", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_shm`.
pub mod wl_shm {
    pub mod req {
        pub const CREATE_POOL: u16 = 0;
        pub const RELEASE: u16 = 1;
    }
    pub mod ev {
        pub const FORMAT: u16 = 0;
    }
}

pub static WL_BUFFER: Interface = Interface {
    name: "wl_buffer",
    version: 1,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "release", since: 1, destructor: false, args: &[] },
    ],
};

/// Los números de las peticiones y eventos de `wl_buffer`.
pub mod wl_buffer {
    pub mod req {
        pub const DESTROY: u16 = 0;
    }
    pub mod ev {
        pub const RELEASE: u16 = 0;
    }
}

pub static WL_DATA_OFFER: Interface = Interface {
    name: "wl_data_offer",
    version: 4,
    requests: &[
        Message { name: "accept", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "mime_type", kind: Kind::Str, nullable: true, interface: None }] },
        Message { name: "receive", since: 1, destructor: false, args: &[Arg { name: "mime_type", kind: Kind::Str, nullable: false, interface: None }, Arg { name: "fd", kind: Kind::Fd, nullable: false, interface: None }] },
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "finish", since: 3, destructor: false, args: &[] },
        Message { name: "set_actions", since: 3, destructor: false, args: &[Arg { name: "dnd_actions", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "preferred_action", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
    events: &[
        Message { name: "offer", since: 1, destructor: false, args: &[Arg { name: "mime_type", kind: Kind::Str, nullable: false, interface: None }] },
        Message { name: "source_actions", since: 3, destructor: false, args: &[Arg { name: "source_actions", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "action", since: 3, destructor: false, args: &[Arg { name: "dnd_action", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_data_offer`.
pub mod wl_data_offer {
    pub mod req {
        pub const ACCEPT: u16 = 0;
        pub const RECEIVE: u16 = 1;
        pub const DESTROY: u16 = 2;
        pub const FINISH: u16 = 3;
        pub const SET_ACTIONS: u16 = 4;
    }
    pub mod ev {
        pub const OFFER: u16 = 0;
        pub const SOURCE_ACTIONS: u16 = 1;
        pub const ACTION: u16 = 2;
    }
}

pub static WL_DATA_SOURCE: Interface = Interface {
    name: "wl_data_source",
    version: 4,
    requests: &[
        Message { name: "offer", since: 1, destructor: false, args: &[Arg { name: "mime_type", kind: Kind::Str, nullable: false, interface: None }] },
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "set_actions", since: 3, destructor: false, args: &[Arg { name: "dnd_actions", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
    events: &[
        Message { name: "target", since: 1, destructor: false, args: &[Arg { name: "mime_type", kind: Kind::Str, nullable: true, interface: None }] },
        Message { name: "send", since: 1, destructor: false, args: &[Arg { name: "mime_type", kind: Kind::Str, nullable: false, interface: None }, Arg { name: "fd", kind: Kind::Fd, nullable: false, interface: None }] },
        Message { name: "cancelled", since: 1, destructor: false, args: &[] },
        Message { name: "dnd_drop_performed", since: 3, destructor: false, args: &[] },
        Message { name: "dnd_finished", since: 3, destructor: false, args: &[] },
        Message { name: "action", since: 3, destructor: false, args: &[Arg { name: "dnd_action", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_data_source`.
pub mod wl_data_source {
    pub mod req {
        pub const OFFER: u16 = 0;
        pub const DESTROY: u16 = 1;
        pub const SET_ACTIONS: u16 = 2;
    }
    pub mod ev {
        pub const TARGET: u16 = 0;
        pub const SEND: u16 = 1;
        pub const CANCELLED: u16 = 2;
        pub const DND_DROP_PERFORMED: u16 = 3;
        pub const DND_FINISHED: u16 = 4;
        pub const ACTION: u16 = 5;
    }
}

pub static WL_DATA_DEVICE: Interface = Interface {
    name: "wl_data_device",
    version: 4,
    requests: &[
        Message { name: "start_drag", since: 1, destructor: false, args: &[Arg { name: "source", kind: Kind::Object, nullable: true, interface: Some("wl_data_source") }, Arg { name: "origin", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }, Arg { name: "icon", kind: Kind::Object, nullable: true, interface: Some("wl_surface") }, Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "set_selection", since: 1, destructor: false, args: &[Arg { name: "source", kind: Kind::Object, nullable: true, interface: Some("wl_data_source") }, Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "release", since: 2, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "data_offer", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_data_offer") }] },
        Message { name: "enter", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }, Arg { name: "x", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "id", kind: Kind::Object, nullable: true, interface: Some("wl_data_offer") }] },
        Message { name: "leave", since: 1, destructor: false, args: &[] },
        Message { name: "motion", since: 1, destructor: false, args: &[Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "x", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Fixed, nullable: false, interface: None }] },
        Message { name: "drop", since: 1, destructor: false, args: &[] },
        Message { name: "selection", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::Object, nullable: true, interface: Some("wl_data_offer") }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_data_device`.
pub mod wl_data_device {
    pub mod req {
        pub const START_DRAG: u16 = 0;
        pub const SET_SELECTION: u16 = 1;
        pub const RELEASE: u16 = 2;
    }
    pub mod ev {
        pub const DATA_OFFER: u16 = 0;
        pub const ENTER: u16 = 1;
        pub const LEAVE: u16 = 2;
        pub const MOTION: u16 = 3;
        pub const DROP: u16 = 4;
        pub const SELECTION: u16 = 5;
    }
}

pub static WL_DATA_DEVICE_MANAGER: Interface = Interface {
    name: "wl_data_device_manager",
    version: 4,
    requests: &[
        Message { name: "create_data_source", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_data_source") }] },
        Message { name: "get_data_device", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_data_device") }, Arg { name: "seat", kind: Kind::Object, nullable: false, interface: Some("wl_seat") }] },
        Message { name: "release", since: 4, destructor: true, args: &[] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `wl_data_device_manager`.
pub mod wl_data_device_manager {
    pub mod req {
        pub const CREATE_DATA_SOURCE: u16 = 0;
        pub const GET_DATA_DEVICE: u16 = 1;
        pub const RELEASE: u16 = 2;
    }
    pub mod ev {
    }
}

pub static WL_SURFACE: Interface = Interface {
    name: "wl_surface",
    version: 7,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "attach", since: 1, destructor: false, args: &[Arg { name: "buffer", kind: Kind::Object, nullable: true, interface: Some("wl_buffer") }, Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "damage", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "frame", since: 1, destructor: false, args: &[Arg { name: "callback", kind: Kind::NewId, nullable: false, interface: Some("wl_callback") }] },
        Message { name: "set_opaque_region", since: 1, destructor: false, args: &[Arg { name: "region", kind: Kind::Object, nullable: true, interface: Some("wl_region") }] },
        Message { name: "set_input_region", since: 1, destructor: false, args: &[Arg { name: "region", kind: Kind::Object, nullable: true, interface: Some("wl_region") }] },
        Message { name: "commit", since: 1, destructor: false, args: &[] },
        Message { name: "set_buffer_transform", since: 2, destructor: false, args: &[Arg { name: "transform", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "set_buffer_scale", since: 3, destructor: false, args: &[Arg { name: "scale", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "damage_buffer", since: 4, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "offset", since: 5, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "get_release", since: 7, destructor: false, args: &[Arg { name: "callback", kind: Kind::NewId, nullable: false, interface: Some("wl_callback") }] },
    ],
    events: &[
        Message { name: "enter", since: 1, destructor: false, args: &[Arg { name: "output", kind: Kind::Object, nullable: false, interface: Some("wl_output") }] },
        Message { name: "leave", since: 1, destructor: false, args: &[Arg { name: "output", kind: Kind::Object, nullable: false, interface: Some("wl_output") }] },
        Message { name: "preferred_buffer_scale", since: 6, destructor: false, args: &[Arg { name: "factor", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "preferred_buffer_transform", since: 6, destructor: false, args: &[Arg { name: "transform", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_surface`.
pub mod wl_surface {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const ATTACH: u16 = 1;
        pub const DAMAGE: u16 = 2;
        pub const FRAME: u16 = 3;
        pub const SET_OPAQUE_REGION: u16 = 4;
        pub const SET_INPUT_REGION: u16 = 5;
        pub const COMMIT: u16 = 6;
        pub const SET_BUFFER_TRANSFORM: u16 = 7;
        pub const SET_BUFFER_SCALE: u16 = 8;
        pub const DAMAGE_BUFFER: u16 = 9;
        pub const OFFSET: u16 = 10;
        pub const GET_RELEASE: u16 = 11;
    }
    pub mod ev {
        pub const ENTER: u16 = 0;
        pub const LEAVE: u16 = 1;
        pub const PREFERRED_BUFFER_SCALE: u16 = 2;
        pub const PREFERRED_BUFFER_TRANSFORM: u16 = 3;
    }
}

pub static WL_SEAT: Interface = Interface {
    name: "wl_seat",
    version: 11,
    requests: &[
        Message { name: "get_pointer", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_pointer") }] },
        Message { name: "get_keyboard", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_keyboard") }] },
        Message { name: "get_touch", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_touch") }] },
        Message { name: "release", since: 5, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "capabilities", since: 1, destructor: false, args: &[Arg { name: "capabilities", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "name", since: 2, destructor: false, args: &[Arg { name: "name", kind: Kind::Str, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_seat`.
pub mod wl_seat {
    pub mod req {
        pub const GET_POINTER: u16 = 0;
        pub const GET_KEYBOARD: u16 = 1;
        pub const GET_TOUCH: u16 = 2;
        pub const RELEASE: u16 = 3;
    }
    pub mod ev {
        pub const CAPABILITIES: u16 = 0;
        pub const NAME: u16 = 1;
    }
}

pub static WL_POINTER: Interface = Interface {
    name: "wl_pointer",
    version: 11,
    requests: &[
        Message { name: "set_cursor", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface", kind: Kind::Object, nullable: true, interface: Some("wl_surface") }, Arg { name: "hotspot_x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "hotspot_y", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "release", since: 3, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "enter", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }, Arg { name: "surface_x", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "surface_y", kind: Kind::Fixed, nullable: false, interface: None }] },
        Message { name: "leave", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }] },
        Message { name: "motion", since: 1, destructor: false, args: &[Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface_x", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "surface_y", kind: Kind::Fixed, nullable: false, interface: None }] },
        Message { name: "button", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "button", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "state", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "axis", since: 1, destructor: false, args: &[Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "axis", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "value", kind: Kind::Fixed, nullable: false, interface: None }] },
        Message { name: "frame", since: 5, destructor: false, args: &[] },
        Message { name: "axis_source", since: 5, destructor: false, args: &[Arg { name: "axis_source", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "axis_stop", since: 5, destructor: false, args: &[Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "axis", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "axis_discrete", since: 5, destructor: false, args: &[Arg { name: "axis", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "discrete", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "axis_value120", since: 8, destructor: false, args: &[Arg { name: "axis", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "value120", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "axis_relative_direction", since: 9, destructor: false, args: &[Arg { name: "axis", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "direction", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "warp", since: 11, destructor: false, args: &[Arg { name: "surface_x", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "surface_y", kind: Kind::Fixed, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_pointer`.
pub mod wl_pointer {
    pub mod req {
        pub const SET_CURSOR: u16 = 0;
        pub const RELEASE: u16 = 1;
    }
    pub mod ev {
        pub const ENTER: u16 = 0;
        pub const LEAVE: u16 = 1;
        pub const MOTION: u16 = 2;
        pub const BUTTON: u16 = 3;
        pub const AXIS: u16 = 4;
        pub const FRAME: u16 = 5;
        pub const AXIS_SOURCE: u16 = 6;
        pub const AXIS_STOP: u16 = 7;
        pub const AXIS_DISCRETE: u16 = 8;
        pub const AXIS_VALUE120: u16 = 9;
        pub const AXIS_RELATIVE_DIRECTION: u16 = 10;
        pub const WARP: u16 = 11;
    }
}

pub static WL_KEYBOARD: Interface = Interface {
    name: "wl_keyboard",
    version: 11,
    requests: &[
        Message { name: "release", since: 3, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "keymap", since: 1, destructor: false, args: &[Arg { name: "format", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "fd", kind: Kind::Fd, nullable: false, interface: None }, Arg { name: "size", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "enter", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }, Arg { name: "keys", kind: Kind::Array, nullable: false, interface: None }] },
        Message { name: "leave", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }] },
        Message { name: "key", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "key", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "state", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "modifiers", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "mods_depressed", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "mods_latched", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "mods_locked", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "group", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "repeat_info", since: 4, destructor: false, args: &[Arg { name: "rate", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "delay", kind: Kind::Int, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_keyboard`.
pub mod wl_keyboard {
    pub mod req {
        pub const RELEASE: u16 = 0;
    }
    pub mod ev {
        pub const KEYMAP: u16 = 0;
        pub const ENTER: u16 = 1;
        pub const LEAVE: u16 = 2;
        pub const KEY: u16 = 3;
        pub const MODIFIERS: u16 = 4;
        pub const REPEAT_INFO: u16 = 5;
    }
}

pub static WL_TOUCH: Interface = Interface {
    name: "wl_touch",
    version: 11,
    requests: &[
        Message { name: "release", since: 3, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "down", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }, Arg { name: "id", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "x", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Fixed, nullable: false, interface: None }] },
        Message { name: "up", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "id", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "motion", since: 1, destructor: false, args: &[Arg { name: "time", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "id", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "x", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Fixed, nullable: false, interface: None }] },
        Message { name: "frame", since: 1, destructor: false, args: &[] },
        Message { name: "cancel", since: 1, destructor: false, args: &[] },
        Message { name: "shape", since: 6, destructor: false, args: &[Arg { name: "id", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "major", kind: Kind::Fixed, nullable: false, interface: None }, Arg { name: "minor", kind: Kind::Fixed, nullable: false, interface: None }] },
        Message { name: "orientation", since: 6, destructor: false, args: &[Arg { name: "id", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "orientation", kind: Kind::Fixed, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_touch`.
pub mod wl_touch {
    pub mod req {
        pub const RELEASE: u16 = 0;
    }
    pub mod ev {
        pub const DOWN: u16 = 0;
        pub const UP: u16 = 1;
        pub const MOTION: u16 = 2;
        pub const FRAME: u16 = 3;
        pub const CANCEL: u16 = 4;
        pub const SHAPE: u16 = 5;
        pub const ORIENTATION: u16 = 6;
    }
}

pub static WL_OUTPUT: Interface = Interface {
    name: "wl_output",
    version: 4,
    requests: &[
        Message { name: "release", since: 3, destructor: true, args: &[] },
    ],
    events: &[
        Message { name: "geometry", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "physical_width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "physical_height", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "subpixel", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "make", kind: Kind::Str, nullable: false, interface: None }, Arg { name: "model", kind: Kind::Str, nullable: false, interface: None }, Arg { name: "transform", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "mode", since: 1, destructor: false, args: &[Arg { name: "flags", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "refresh", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "done", since: 2, destructor: false, args: &[] },
        Message { name: "scale", since: 2, destructor: false, args: &[Arg { name: "factor", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "name", since: 4, destructor: false, args: &[Arg { name: "name", kind: Kind::Str, nullable: false, interface: None }] },
        Message { name: "description", since: 4, destructor: false, args: &[Arg { name: "description", kind: Kind::Str, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `wl_output`.
pub mod wl_output {
    pub mod req {
        pub const RELEASE: u16 = 0;
    }
    pub mod ev {
        pub const GEOMETRY: u16 = 0;
        pub const MODE: u16 = 1;
        pub const DONE: u16 = 2;
        pub const SCALE: u16 = 3;
        pub const NAME: u16 = 4;
        pub const DESCRIPTION: u16 = 5;
    }
}

pub static WL_REGION: Interface = Interface {
    name: "wl_region",
    version: 7,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "add", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "subtract", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `wl_region`.
pub mod wl_region {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const ADD: u16 = 1;
        pub const SUBTRACT: u16 = 2;
    }
    pub mod ev {
    }
}

pub static WL_SUBCOMPOSITOR: Interface = Interface {
    name: "wl_subcompositor",
    version: 1,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "get_subsurface", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("wl_subsurface") }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }, Arg { name: "parent", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `wl_subcompositor`.
pub mod wl_subcompositor {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const GET_SUBSURFACE: u16 = 1;
    }
    pub mod ev {
    }
}

pub static WL_SUBSURFACE: Interface = Interface {
    name: "wl_subsurface",
    version: 1,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "set_position", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "place_above", since: 1, destructor: false, args: &[Arg { name: "sibling", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }] },
        Message { name: "place_below", since: 1, destructor: false, args: &[Arg { name: "sibling", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }] },
        Message { name: "set_sync", since: 1, destructor: false, args: &[] },
        Message { name: "set_desync", since: 1, destructor: false, args: &[] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `wl_subsurface`.
pub mod wl_subsurface {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const SET_POSITION: u16 = 1;
        pub const PLACE_ABOVE: u16 = 2;
        pub const PLACE_BELOW: u16 = 3;
        pub const SET_SYNC: u16 = 4;
        pub const SET_DESYNC: u16 = 5;
    }
    pub mod ev {
    }
}

pub static WL_FIXES: Interface = Interface {
    name: "wl_fixes",
    version: 2,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "destroy_registry", since: 1, destructor: false, args: &[Arg { name: "registry", kind: Kind::Object, nullable: false, interface: Some("wl_registry") }] },
        Message { name: "ack_global_remove", since: 2, destructor: false, args: &[Arg { name: "registry", kind: Kind::Object, nullable: false, interface: Some("wl_registry") }, Arg { name: "name", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `wl_fixes`.
pub mod wl_fixes {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const DESTROY_REGISTRY: u16 = 1;
        pub const ACK_GLOBAL_REMOVE: u16 = 2;
    }
    pub mod ev {
    }
}

pub static XDG_WM_BASE: Interface = Interface {
    name: "xdg_wm_base",
    version: 7,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "create_positioner", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("xdg_positioner") }] },
        Message { name: "get_xdg_surface", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("xdg_surface") }, Arg { name: "surface", kind: Kind::Object, nullable: false, interface: Some("wl_surface") }] },
        Message { name: "pong", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
    events: &[
        Message { name: "ping", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `xdg_wm_base`.
pub mod xdg_wm_base {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const CREATE_POSITIONER: u16 = 1;
        pub const GET_XDG_SURFACE: u16 = 2;
        pub const PONG: u16 = 3;
    }
    pub mod ev {
        pub const PING: u16 = 0;
    }
}

pub static XDG_POSITIONER: Interface = Interface {
    name: "xdg_positioner",
    version: 7,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "set_size", since: 1, destructor: false, args: &[Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "set_anchor_rect", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "set_anchor", since: 1, destructor: false, args: &[Arg { name: "anchor", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "set_gravity", since: 1, destructor: false, args: &[Arg { name: "gravity", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "set_constraint_adjustment", since: 1, destructor: false, args: &[Arg { name: "constraint_adjustment", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "set_offset", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "set_reactive", since: 3, destructor: false, args: &[] },
        Message { name: "set_parent_size", since: 3, destructor: false, args: &[Arg { name: "parent_width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "parent_height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "set_parent_configure", since: 3, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
    events: &[

    ],
};

/// Los números de las peticiones y eventos de `xdg_positioner`.
pub mod xdg_positioner {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const SET_SIZE: u16 = 1;
        pub const SET_ANCHOR_RECT: u16 = 2;
        pub const SET_ANCHOR: u16 = 3;
        pub const SET_GRAVITY: u16 = 4;
        pub const SET_CONSTRAINT_ADJUSTMENT: u16 = 5;
        pub const SET_OFFSET: u16 = 6;
        pub const SET_REACTIVE: u16 = 7;
        pub const SET_PARENT_SIZE: u16 = 8;
        pub const SET_PARENT_CONFIGURE: u16 = 9;
    }
    pub mod ev {
    }
}

pub static XDG_SURFACE: Interface = Interface {
    name: "xdg_surface",
    version: 7,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "get_toplevel", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("xdg_toplevel") }] },
        Message { name: "get_popup", since: 1, destructor: false, args: &[Arg { name: "id", kind: Kind::NewId, nullable: false, interface: Some("xdg_popup") }, Arg { name: "parent", kind: Kind::Object, nullable: true, interface: Some("xdg_surface") }, Arg { name: "positioner", kind: Kind::Object, nullable: false, interface: Some("xdg_positioner") }] },
        Message { name: "set_window_geometry", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "ack_configure", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
    events: &[
        Message { name: "configure", since: 1, destructor: false, args: &[Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `xdg_surface`.
pub mod xdg_surface {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const GET_TOPLEVEL: u16 = 1;
        pub const GET_POPUP: u16 = 2;
        pub const SET_WINDOW_GEOMETRY: u16 = 3;
        pub const ACK_CONFIGURE: u16 = 4;
    }
    pub mod ev {
        pub const CONFIGURE: u16 = 0;
    }
}

pub static XDG_TOPLEVEL: Interface = Interface {
    name: "xdg_toplevel",
    version: 7,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "set_parent", since: 1, destructor: false, args: &[Arg { name: "parent", kind: Kind::Object, nullable: true, interface: Some("xdg_toplevel") }] },
        Message { name: "set_title", since: 1, destructor: false, args: &[Arg { name: "title", kind: Kind::Str, nullable: false, interface: None }] },
        Message { name: "set_app_id", since: 1, destructor: false, args: &[Arg { name: "app_id", kind: Kind::Str, nullable: false, interface: None }] },
        Message { name: "show_window_menu", since: 1, destructor: false, args: &[Arg { name: "seat", kind: Kind::Object, nullable: false, interface: Some("wl_seat") }, Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "move", since: 1, destructor: false, args: &[Arg { name: "seat", kind: Kind::Object, nullable: false, interface: Some("wl_seat") }, Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "resize", since: 1, destructor: false, args: &[Arg { name: "seat", kind: Kind::Object, nullable: false, interface: Some("wl_seat") }, Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }, Arg { name: "edges", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "set_max_size", since: 1, destructor: false, args: &[Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "set_min_size", since: 1, destructor: false, args: &[Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "set_maximized", since: 1, destructor: false, args: &[] },
        Message { name: "unset_maximized", since: 1, destructor: false, args: &[] },
        Message { name: "set_fullscreen", since: 1, destructor: false, args: &[Arg { name: "output", kind: Kind::Object, nullable: true, interface: Some("wl_output") }] },
        Message { name: "unset_fullscreen", since: 1, destructor: false, args: &[] },
        Message { name: "set_minimized", since: 1, destructor: false, args: &[] },
    ],
    events: &[
        Message { name: "configure", since: 1, destructor: false, args: &[Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "states", kind: Kind::Array, nullable: false, interface: None }] },
        Message { name: "close", since: 1, destructor: false, args: &[] },
        Message { name: "configure_bounds", since: 4, destructor: false, args: &[Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "wm_capabilities", since: 5, destructor: false, args: &[Arg { name: "capabilities", kind: Kind::Array, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `xdg_toplevel`.
pub mod xdg_toplevel {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const SET_PARENT: u16 = 1;
        pub const SET_TITLE: u16 = 2;
        pub const SET_APP_ID: u16 = 3;
        pub const SHOW_WINDOW_MENU: u16 = 4;
        pub const MOVE: u16 = 5;
        pub const RESIZE: u16 = 6;
        pub const SET_MAX_SIZE: u16 = 7;
        pub const SET_MIN_SIZE: u16 = 8;
        pub const SET_MAXIMIZED: u16 = 9;
        pub const UNSET_MAXIMIZED: u16 = 10;
        pub const SET_FULLSCREEN: u16 = 11;
        pub const UNSET_FULLSCREEN: u16 = 12;
        pub const SET_MINIMIZED: u16 = 13;
    }
    pub mod ev {
        pub const CONFIGURE: u16 = 0;
        pub const CLOSE: u16 = 1;
        pub const CONFIGURE_BOUNDS: u16 = 2;
        pub const WM_CAPABILITIES: u16 = 3;
    }
}

pub static XDG_POPUP: Interface = Interface {
    name: "xdg_popup",
    version: 7,
    requests: &[
        Message { name: "destroy", since: 1, destructor: true, args: &[] },
        Message { name: "grab", since: 1, destructor: false, args: &[Arg { name: "seat", kind: Kind::Object, nullable: false, interface: Some("wl_seat") }, Arg { name: "serial", kind: Kind::Uint, nullable: false, interface: None }] },
        Message { name: "reposition", since: 3, destructor: false, args: &[Arg { name: "positioner", kind: Kind::Object, nullable: false, interface: Some("xdg_positioner") }, Arg { name: "token", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
    events: &[
        Message { name: "configure", since: 1, destructor: false, args: &[Arg { name: "x", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "y", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "width", kind: Kind::Int, nullable: false, interface: None }, Arg { name: "height", kind: Kind::Int, nullable: false, interface: None }] },
        Message { name: "popup_done", since: 1, destructor: false, args: &[] },
        Message { name: "repositioned", since: 3, destructor: false, args: &[Arg { name: "token", kind: Kind::Uint, nullable: false, interface: None }] },
    ],
};

/// Los números de las peticiones y eventos de `xdg_popup`.
pub mod xdg_popup {
    pub mod req {
        pub const DESTROY: u16 = 0;
        pub const GRAB: u16 = 1;
        pub const REPOSITION: u16 = 2;
    }
    pub mod ev {
        pub const CONFIGURE: u16 = 0;
        pub const POPUP_DONE: u16 = 1;
        pub const REPOSITIONED: u16 = 2;
    }
}

/// Todas las interfaces, para buscarlas por nombre.
pub static INTERFACES: &[&Interface] = &[
    &WL_DISPLAY,
    &WL_REGISTRY,
    &WL_CALLBACK,
    &WL_COMPOSITOR,
    &WL_SHM_POOL,
    &WL_SHM,
    &WL_BUFFER,
    &WL_DATA_OFFER,
    &WL_DATA_SOURCE,
    &WL_DATA_DEVICE,
    &WL_DATA_DEVICE_MANAGER,
    &WL_SURFACE,
    &WL_SEAT,
    &WL_POINTER,
    &WL_KEYBOARD,
    &WL_TOUCH,
    &WL_OUTPUT,
    &WL_REGION,
    &WL_SUBCOMPOSITOR,
    &WL_SUBSURFACE,
    &WL_FIXES,
    &XDG_WM_BASE,
    &XDG_POSITIONER,
    &XDG_SURFACE,
    &XDG_TOPLEVEL,
    &XDG_POPUP,
];
