// SPDX-License-Identifier: MPL-2.0

mod app;

fn main() -> cosmic::iced::Result {
	// Starts the applet's event loop with `()` as the application's flags.
	cosmic::applet::run::<app::AppModel>(())
}
