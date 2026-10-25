// SPDX-License-Identifier: GPL-3.0-only
//! SPK-2 (GNOME fallback mechanics): an override-redirect, click-through
//! fullscreen X11 window.
//!
//! This is the same client path used against XWayland. This environment has no
//! Wayland compositor, so the run below is an X11 proxy, not an XWayland test.
//!
//! Run from `spikes/spk2-xwayland` with `cargo run --release`.
//! The window stays for five seconds. It prints the window id and the number of
//! rectangles in the input shape (zero means click-through).

use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::shape;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen_number) = RustConnection::connect(None)?;
    let screen = &connection.setup().roots[screen_number];
    let root = screen.root;
    let width = screen.width_in_pixels;
    let height = screen.height_in_pixels;

    let window = connection.generate_id()?;
    connection.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        width,
        height,
        0,
        xproto::WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &xproto::CreateWindowAux::new()
            .override_redirect(1)
            .background_pixel(0x000000)
            .event_mask(xproto::EventMask::EXPOSURE | xproto::EventMask::STRUCTURE_NOTIFY),
    )?;

    // An empty input region is the X11 mechanism for click-through.
    shape::rectangles(
        &connection,
        shape::SO::SET,
        shape::SK::INPUT,
        xproto::ClipOrdering::UNSORTED,
        window,
        0,
        0,
        &[],
    )?;

    connection.change_property8(
        xproto::PropMode::REPLACE,
        window,
        xproto::AtomEnum::WM_NAME,
        xproto::AtomEnum::STRING,
        b"spk2-overlay",
    )?;

    let gc = connection.generate_id()?;
    connection.create_gc(
        gc,
        window,
        &xproto::CreateGCAux::new().foreground(0x00ff8800),
    )?;

    let radius = 80_i16;
    let center_x = i16::try_from(width / 2).unwrap_or(i16::MAX);
    let center_y = i16::try_from(height / 2).unwrap_or(i16::MAX);
    connection.poly_fill_arc(
        gc,
        window,
        &[xproto::Arc {
            x: center_x - radius,
            y: center_y - radius,
            width: u16::try_from(radius * 2).unwrap_or(u16::MAX),
            height: u16::try_from(radius * 2).unwrap_or(u16::MAX),
            angle1: 0,
            angle2: 360 * 64,
        }],
    )?;

    connection.map_window(window)?;
    connection.flush()?;

    let input_rectangles = shape::get_rectangles(&connection, window, shape::SK::INPUT)?
        .reply()?
        .rectangles
        .len();
    println!("window=0x{window:x} screen={width}x{height} input_rectangles={input_rectangles}");

    std::thread::sleep(Duration::from_secs(5));

    connection.destroy_window(window)?;
    connection.flush()?;
    Ok(())
}
