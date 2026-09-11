// SPDX-License-Identifier: MPL-2.0

use std::ops::Div;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveTime, Weekday};
use cosmic::app::{Core, Task};
use cosmic::iced::{time, Subscription};
use cosmic::widget::{self, autosize};
use cosmic::{Application, Element};
use once_cell::sync::Lazy;

// Panel applets are separate layer-shell windows that do NOT auto-resize to
// their content by default — unlike the fixed-size icon buttons most
// applets use, our content's width varies (a few digits vs. a beer emoji),
// so we need to explicitly wrap the view in `autosize` and give it a
// stable id so iced can track the same autosized window across redraws.
static AUTOSIZE_ID: Lazy<cosmic::widget::Id> =
	Lazy::new(|| cosmic::widget::Id::new("friday-countdown-autosize"));

/// Unique app id. Must match the `Exec`/`StartupWMClass` in the .desktop
/// file so cosmic-panel and the applet picker can find this applet.
const APP_ID: &str = "dk.bjarkebjarke.CosmicAppletFridayCountdown";

/// The hour (24h, local time) the countdown targets. 14 == 2 PM.
const TARGET_HOUR: u32 = 14;

/// How often the clock is re-checked. A minute-resolution display doesn't
/// need anything faster, but 10s keeps the switch-over to 🍺 feeling snappy.
const TICK: Duration = Duration::from_secs(10);

pub struct AppModel {
	core: Core,
	now: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub enum Message {
	Tick,
}

impl Application for AppModel {
	type Executor = cosmic::executor::Default;
	type Flags = ();
	type Message = Message;

	const APP_ID: &'static str = APP_ID;

	fn core(&self) -> &Core {
		&self.core
	}

	fn core_mut(&mut self) -> &mut Core {
		&mut self.core
	}

	fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
		let app = AppModel {
			core,
			now: Local::now(),
		};
		(app, Task::none())
	}

	fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
		match message {
			Message::Tick => self.now = Local::now(),
		}
		Task::none()
	}

	fn view(&self) -> Element<'_, Self::Message> {
		// Only ever show anything on Friday. Every other day this renders
		// a zero-size element, so the applet takes up no room in the panel.
		let content: Element<Self::Message> = if self.now.weekday() != Weekday::Fri {
			widget::Space::new().into()
		} else {
			let label = countdown_label(self.now);
			self.core.applet.text(label).into()
		};

		// Without this, the applet's layer-shell window stays at its
		// default size instead of growing to fit the text/emoji, which is
		// why nothing appeared to render.
		autosize::autosize(content, AUTOSIZE_ID.clone()).into()
	}

	fn subscription(&self) -> Subscription<Self::Message> {
		time::every(TICK).map(|_| Message::Tick)
	}
}

/// Minutes remaining until 2 PM, or a beer emoji once 2 PM has passed.
fn countdown_label(now: DateTime<Local>) -> String {
	let target = NaiveTime::from_hms_opt(TARGET_HOUR, 0, 0).expect("valid time");

	if now.time() >= target {
		return "🍺".to_string();
	}

	let target_today = now.date_naive().and_time(target);
	let minutes_left = (target_today - now.naive_local())
		.as_seconds_f32()
		.max(0_f32)
		.div(60_f32)
		.ceil() as u32;

	if minutes_left == 1 {
		format!("({minutes_left} min)")
	} else {
		format!("({minutes_left} mins)")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use chrono::TimeZone;

	fn at(h: u32, m: u32, s: u32) -> DateTime<Local> {
		Local.with_ymd_and_hms(2026, 9, 11, h, m, s).unwrap()
	}

	#[test]
	fn counts_down_before_2pm() {
		assert_eq!(countdown_label(at(13, 45, 0)), "(15 mins)");
		assert_eq!(countdown_label(at(13, 59, 0)), "(1 min)");
		assert_eq!(countdown_label(at(9, 0, 0)), "(300 mins)");
		assert_eq!(countdown_label(at(13, 45, 30)), "(15 mins)");
	}

	#[test]
	fn shows_beer_at_and_after_2pm() {
		assert_eq!(countdown_label(at(14, 0, 0)), "🍺");
		assert_eq!(countdown_label(at(23, 59, 0)), "🍺");
	}
}
