// SPDX-License-Identifier: MPL-2.0

use std::ops::Div;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveTime, Timelike, Weekday};
use cosmic::app::{Core, Task};
use cosmic::iced::futures::Stream;
use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::futures::sink::SinkExt;
use cosmic::iced::{Subscription, stream};
use cosmic::widget;
use cosmic::{Application, Element};

/// Unique app id. Must match the `Exec`/`StartupWMClass` in the .desktop
/// file so cosmic-panel and the applet picker can find this applet.
const APP_ID: &str = "dk.bjarkebjarke.CosmicAppletFridayCountdown";

/// The hour (24h, local time) the countdown targets. 14 == 2 PM.
const TARGET_HOUR: u32 = 14;

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
		let content: Element<'_, Self::Message> = if self.now.weekday() != Weekday::Fri {
			widget::Space::new().into()
		} else {
			let label = countdown_label(self.now);
			self.core.applet.text(label).into()
		};

		// `Context::autosize_window` (not a bare `autosize` free function)
		// is the applet-aware version: besides resizing the layer-shell
		// window to fit the content, it also clamps to the panel's own
		// suggested size limits. Without it the applet's window stays at
		// its default size instead of growing to fit the text/emoji.
		self.core.applet.autosize_window(content).into()
	}

	fn subscription(&self) -> Subscription<Self::Message> {
		// `Subscription::run` takes a bare fn pointer (no captures needed
		// here) that builds the stream; iced/cosmic use the pointer itself
		// to identify the subscription, so no separate id is needed.
		Subscription::run(minute_tick_stream)
	}
}

/// A stream that yields `Message::Tick` right at the top of every minute —
/// the same instant COSMIC's own clock applet updates — by sleeping for
/// exactly the remaining time each cycle instead of polling at a fixed
/// interval that would slowly drift out of phase with it.
fn minute_tick_stream() -> impl Stream<Item = Message> {
	stream::channel(1, |mut output: mpsc::Sender<Message>| async move {
		loop {
			let now = Local::now();
			// Remaining time to the next :00, including the fractional
			// second, so each wakeup lands as close to the boundary as
			// possible rather than drifting a little further each cycle.
			let secs_left = 59 - now.second().min(59) as u64;
			let sleep_for = Duration::from_secs(secs_left) + Duration::from_secs(1)
				- Duration::from_nanos(u64::from(now.timestamp_subsec_nanos()));

			tokio::time::sleep(sleep_for).await;

			// If the receiving end is gone the applet is shutting down;
			// ignore the error and let the loop (and process) end naturally.
			let _ = output.send(Message::Tick).await;
		}
	})
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
