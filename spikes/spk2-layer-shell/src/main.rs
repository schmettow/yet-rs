// SPDX-License-Identifier: GPL-3.0-only
//! SPK-2 (Wayland path): probe whether the compositor advertises
//! `zwlr_layer_shell_v1` through the chosen Smithay client toolkit stack.
//!
//! There is no Wayland compositor in the current environment. This binary
//! therefore verifies the SCTK API and protocol bindings at compile time; the
//! runtime check must be repeated on KDE, wlroots or Sway.

use std::error::Error;

use smithay_client_toolkit::{
    delegate_registry,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
};
use wayland_client::{globals::registry_queue_init, Connection};

struct State {
    registry_state: RegistryState,
}

delegate_registry!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers! {}
}

fn main() -> Result<(), Box<dyn Error>> {
    let connection = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init::<State>(&connection)?;
    let registry_state = RegistryState::new(&globals);
    let mut state = State { registry_state };
    event_queue.roundtrip(&mut state)?;

    let has_layer_shell = globals
        .contents()
        .clone_list()
        .iter()
        .any(|global| global.interface == "zwlr_layer_shell_v1");

    println!("layer_shell={has_layer_shell}");
    Ok(())
}
