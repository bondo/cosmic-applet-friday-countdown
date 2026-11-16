// SPDX-License-Identifier: MPL-2.0

use std::pin::pin;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeDelta, TimeZone, Timelike, Weekday};
use cosmic::app::{Core, Task};
use cosmic::iced::Subscription;
use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::futures::future::{self, Either};
use cosmic::iced::futures::sink::SinkExt;
use cosmic::iced::futures::{Stream, StreamExt, stream as futures_stream};
use cosmic::widget;
use cosmic::{Application, Element};

/// Unique app id: the .desktop file is named after it (panels list applets by
/// that name), and the icon is `<APP_ID>-symbolic`. The desktop entry also
/// sets `StartupWMClass` to it.
const APP_ID: &str = "dk.bjarkebjarke.CosmicAppletFridayCountdown";

/// The hour (24h, local time) the countdown targets. 14 == 2 PM.
const TARGET_HOUR: u32 = 14;
// 0 would leave nothing to count down (and the tests need an hour before it).
const _: () = assert!(
	TARGET_HOUR >= 1 && TARGET_HOUR <= 23,
	"TARGET_HOUR must be 1-23"
);

/// Shown on Friday from [`TARGET_HOUR`] on.
const BEER: &str = "🍺";

pub struct AppModel {
	core: Core,
	now: DateTime<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
		// Only show anything on Friday. Other days render an empty element; the
		// panel still gives the applet its smallest window (1×1 logical px).
		let content: Element<'_, Self::Message> = match label(self.now) {
			Some(label) => self.core.applet.text(label).into(),
			None => widget::Space::new().into(),
		};

		// Applet-aware autosize: fits the window to the content within the
		// panel's suggested size limits.
		self.core.applet.autosize_window(content).into()
	}

	fn subscription(&self) -> Subscription<Self::Message> {
		// iced identifies the subscription by the fn pointer.
		Subscription::run(tick_stream)
	}
}

/// Yields `Message::Tick` at the top of every minute and on every logind
/// `PrepareForSleep` signal, notably right after the machine wakes from
/// suspend.
fn tick_stream() -> impl Stream<Item = Message> {
	logind_ticks(Local::now, zbus::Connection::system())
}

/// [`tick_stream`] with the wall clock and the D-Bus connection to logind
/// injected, so tests can drive it with a fake clock and a fake logind.
fn logind_ticks<Tz: TimeZone>(
	now: impl Fn() -> DateTime<Tz>,
	connection: impl Future<Output = zbus::Result<zbus::Connection>>,
) -> impl Stream<Item = Message> {
	ticks(now, sleep_signals(logind_prepare_for_sleep(connection)))
}

/// Ticks at every wall-clock minute boundary and on every item of
/// `refreshes`.
///
/// Each cycle sleeps until the next minute boundary, recomputed every time so
/// it doesn't drift. The sleep runs on tokio's monotonic clock, which stops
/// during suspend, so a wake-up (an item of `refreshes`) both ticks and
/// restarts the sleep from the new wall-clock time; otherwise the first minute
/// change after resume could be up to a minute late.
fn ticks<Tz: TimeZone>(
	now: impl Fn() -> DateTime<Tz>,
	refreshes: impl Stream<Item = ()>,
) -> impl Stream<Item = Message> {
	cosmic::iced::stream::channel(1, async move |mut output: mpsc::Sender<Message>| {
		// Once `refreshes` ends (e.g. no logind), `pending` takes over and never
		// yields, so only the minute sleep can finish and the ended stream isn't
		// polled again.
		let mut refreshes = pin!(refreshes.chain(futures_stream::pending()));
		loop {
			let sleep = pin!(tokio::time::sleep(until_next_minute(now())));
			// A minute boundary, or a refresh: tick.
			future::select(sleep, refreshes.next()).await;
			// Only fails while iced tears the subscription down, which drops
			// this loop too.
			let _ = output.send(Message::Tick).await;
		}
	})
}

/// logind's Manager interface, reduced to the one signal we need.
#[zbus::proxy(
	interface = "org.freedesktop.login1.Manager",
	default_service = "org.freedesktop.login1",
	default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
	/// Sent with `start == true` before suspending and `false` after waking.
	#[zbus(signal)]
	fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

/// One item per logind `PrepareForSleep` signal received over `connection`.
///
/// Its `start` argument (going to sleep or waking up) is ignored: an extra
/// refresh right before suspending is harmless.
async fn logind_prepare_for_sleep(
	connection: impl Future<Output = zbus::Result<zbus::Connection>>,
) -> zbus::Result<impl Stream<Item = ()>> {
	let connection = connection.await?;
	let proxy = Login1ManagerProxy::new(&connection).await?;
	let signals = proxy.receive_prepare_for_sleep().await?;
	Ok(signals.map(|_| ()))
}

/// The signals from `signal_source` once it is set up. If it can't be set up
/// (no system bus, no logind), logs to stderr and ends: not fatal, [`ticks`]
/// then only ticks on minute boundaries.
fn sleep_signals<S: Stream<Item = ()>>(
	signal_source: impl Future<Output = zbus::Result<S>>,
) -> impl Stream<Item = ()> {
	futures_stream::once(signal_source).flat_map(|signals| match signals {
		Ok(signals) => Either::Left(signals),
		Err(err) => {
			eprintln!("friday-countdown: not refreshing on wake from suspend: {err}");
			Either::Right(futures_stream::empty())
		}
	})
}

/// Time from `now` until the next wall-clock minute boundary (:00).
/// Always positive, so the ticker never spins.
fn until_next_minute<Tz: TimeZone>(now: DateTime<Tz>) -> Duration {
	// chrono represents a leap second as second 59 with >= 1e9 nanos; clamp
	// the nanos so the subtraction below can't underflow.
	let into_minute = Duration::from_secs(u64::from(now.second()))
		+ Duration::from_nanos(u64::from(now.nanosecond().min(999_999_999)));
	Duration::from_secs(60) - into_minute
}

/// What the applet shows at `now`: nothing (`None`) unless it's Friday, then
/// the minutes left until [`TARGET_HOUR`], or a beer emoji from then on.
fn label<Tz: TimeZone>(now: DateTime<Tz>) -> Option<String> {
	label_for_target(now, TARGET_HOUR)
}

/// [`label`] with the target hour as a parameter, so tests can put it in a
/// DST gap or overlap.
fn label_for_target<Tz: TimeZone>(now: DateTime<Tz>, target_hour: u32) -> Option<String> {
	if now.weekday() != Weekday::Fri {
		return None;
	}

	let target_time = NaiveTime::from_hms_opt(target_hour, 0, 0).expect("valid target hour");

	// Subtract real instants, not wall-clock times, so a DST change between
	// now and the target (e.g. Asia/Jerusalem switches on a Friday) doesn't
	// put the count off by an hour. From the target on, it's beer time.
	// - DST gap (the target time doesn't exist that day): count to it in
	//   wall-clock time. Once the clocks jump past it, it's beer time.
	// - DST overlap (the target time happens twice): its first occurrence is
	//   the target. If that has passed (we're in the repeated hour, before the
	//   target's wall-clock time comes round again), it's beer time.
	let naive_target = now.date_naive().and_time(target_time);
	let remaining = match now.timezone().from_local_datetime(&naive_target).earliest() {
		Some(target) => target.signed_duration_since(&now),
		None => naive_target - now.naive_local(),
	};
	if remaining <= TimeDelta::zero() {
		return Some(BEER.to_string());
	}

	// Round partial minutes up, to the nanosecond: 13:58:00.000000001 shows
	// "(2 mins)".
	let nanos = remaining.to_std().expect("positive").as_nanos();
	let minutes_left = nanos.div_ceil(60_000_000_000);
	Some(if minutes_left == 1 {
		"(1 min)".to_string()
	} else {
		format!("({minutes_left} mins)")
	})
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicI64, Ordering};

	use chrono::{FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime};
	use chrono_tz::Tz;
	use cosmic::iced::advanced::subscription::{Hasher, into_recipes};

	use super::*;

	const COPENHAGEN: Tz = chrono_tz::Europe::Copenhagen;

	/// 2026-09-11 is a Friday.
	fn friday(h: u32, m: u32, s: u32, ms: u32) -> DateTime<Tz> {
		COPENHAGEN
			.with_ymd_and_hms(2026, 9, 11, h, m, s)
			.unwrap()
			.with_nanosecond(ms * 1_000_000)
			.unwrap()
	}

	// --- label ---
	//
	// These follow `TARGET_HOUR`, so changing it doesn't break them (as long as
	// there is an hour before it to count down in).

	/// The hour before `TARGET_HOUR`.
	const LAST_HOUR: u32 = TARGET_HOUR - 1;

	#[test]
	fn counts_down_before_target_hour() {
		let in_last_hour = |m, s| label(friday(LAST_HOUR, m, s, 0));
		assert_eq!(in_last_hour(45, 0).as_deref(), Some("(15 mins)"));
		assert_eq!(in_last_hour(45, 30).as_deref(), Some("(15 mins)"));
		assert_eq!(in_last_hour(59, 0).as_deref(), Some("(1 min)"));
		for hour in 0..TARGET_HOUR {
			let expected = format!("({} mins)", (TARGET_HOUR - hour) * 60);
			assert_eq!(label(friday(hour, 0, 0, 0)), Some(expected), "{hour}:00");
		}
	}

	#[test]
	fn rounds_partial_minutes_up() {
		let just_after = friday(LAST_HOUR, 58, 0, 0).with_nanosecond(1).unwrap();
		assert_eq!(label(just_after).as_deref(), Some("(2 mins)"));
		// One nanosecond to go.
		let almost = friday(LAST_HOUR, 59, 59, 0)
			.with_nanosecond(999_999_999)
			.unwrap();
		assert_eq!(label(almost).as_deref(), Some("(1 min)"));
	}

	#[test]
	fn shows_beer_from_target_hour_on() {
		assert_eq!(label(friday(TARGET_HOUR, 0, 0, 0)).as_deref(), Some(BEER));
		assert_eq!(label(friday(23, 59, 59, 999)).as_deref(), Some(BEER));
	}

	#[test]
	fn hidden_on_other_days() {
		// The last millisecond of Thursday and the first of Saturday.
		let thursday = friday(0, 0, 0, 0) - TimeDelta::milliseconds(1);
		let saturday = COPENHAGEN.with_ymd_and_hms(2026, 9, 12, 0, 0, 0).unwrap();
		assert_eq!(thursday.weekday(), Weekday::Thu);
		assert_eq!(label(thursday), None);
		assert_eq!(label(saturday), None);
		// Every other weekday, Saturday 2026-09-12 to Thursday 2026-09-17.
		for day in 12..=17 {
			let t = COPENHAGEN.with_ymd_and_hms(2026, 9, day, 9, 0, 0).unwrap();
			assert_ne!(t.weekday(), Weekday::Fri);
			assert_eq!(label(t), None, "{t}");
		}
	}

	#[test]
	fn counts_real_time_across_dst_change() {
		// Asia/Jerusalem starts DST on Friday 2026-03-27 at 02:00 (clocks
		// jump to 03:00), so 01:00 -> 14:00 is 12 real hours, not 13.
		let jerusalem = chrono_tz::Asia::Jerusalem;
		let t = jerusalem.with_ymd_and_hms(2026, 3, 27, 1, 0, 0).unwrap();
		assert_eq!(t.weekday(), Weekday::Fri);
		assert_eq!(label_for_target(t, 14).as_deref(), Some("(720 mins)"));
		// After the jump, wall-clock and real time agree again.
		let t = jerusalem.with_ymd_and_hms(2026, 3, 27, 3, 0, 0).unwrap();
		assert_eq!(label_for_target(t, 14).as_deref(), Some("(660 mins)"));
	}

	#[test]
	fn dst_gap_target_counts_wall_clock_then_shows_beer() {
		// On Friday 2026-03-27 Asia/Jerusalem jumps from 02:00 to 03:00, so a
		// 02:00 target doesn't exist that day.
		let jerusalem = chrono_tz::Asia::Jerusalem;
		let before = jerusalem.with_ymd_and_hms(2026, 3, 27, 1, 30, 0).unwrap();
		let after = jerusalem.with_ymd_and_hms(2026, 3, 27, 3, 0, 0).unwrap();
		assert_eq!(label_for_target(before, 2).as_deref(), Some("(30 mins)"));
		assert_eq!(label_for_target(after, 2).as_deref(), Some(BEER));
	}

	/// A made-up zone that falls back two hours on Friday 2026-09-11, from
	/// UTC+3 to UTC+1 at 02:00 local time, so local times from 00:00 to 01:59
	/// happen twice. (No real zone moves the clocks back on a Friday with the
	/// target hour inside the repeated part.)
	#[derive(Clone, Copy, Debug)]
	struct FallsBackTwoHours;

	impl FallsBackTwoHours {
		fn transition_utc() -> NaiveDateTime {
			NaiveDate::from_ymd_opt(2026, 9, 10)
				.unwrap()
				.and_hms_opt(23, 0, 0)
				.unwrap()
		}

		fn before() -> FixedOffset {
			FixedOffset::east_opt(3 * 3600).unwrap()
		}

		fn after() -> FixedOffset {
			FixedOffset::east_opt(3600).unwrap()
		}
	}

	impl TimeZone for FallsBackTwoHours {
		type Offset = FixedOffset;

		fn from_offset(_: &FixedOffset) -> Self {
			Self
		}

		fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<FixedOffset> {
			self.offset_from_local_datetime(&local.and_time(NaiveTime::MIN))
		}

		fn offset_from_local_datetime(
			&self,
			local: &NaiveDateTime,
		) -> MappedLocalTime<FixedOffset> {
			let (before, after) = (Self::before(), Self::after());
			let valid_before = *local - before < Self::transition_utc();
			let valid_after = *local - after >= Self::transition_utc();
			match (valid_before, valid_after) {
				(true, true) => MappedLocalTime::Ambiguous(before, after),
				(true, false) => MappedLocalTime::Single(before),
				(false, true) => MappedLocalTime::Single(after),
				(false, false) => MappedLocalTime::None,
			}
		}

		fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
			self.offset_from_utc_datetime(&utc.and_time(NaiveTime::MIN))
		}

		fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
			if *utc < Self::transition_utc() {
				Self::before()
			} else {
				Self::after()
			}
		}
	}

	#[test]
	fn dst_overlap_target_is_its_first_occurrence() {
		let half_past_midnight = NaiveDate::from_ymd_opt(2026, 9, 11)
			.unwrap()
			.and_hms_opt(0, 30, 0)
			.unwrap();
		let (first_pass, second_pass) =
			match FallsBackTwoHours.from_local_datetime(&half_past_midnight) {
				MappedLocalTime::Ambiguous(first, second) => (first, second),
				other => panic!("expected an ambiguous time, got {other:?}"),
			};
		assert_eq!(first_pass.weekday(), Weekday::Fri);
		// First pass: 30 real minutes until the first 01:00 (not 150 until the
		// second one).
		assert_eq!(
			label_for_target(first_pass, 1).as_deref(),
			Some("(30 mins)")
		);
		// Second pass: the first 01:00 was 90 real minutes ago.
		assert_eq!(label_for_target(second_pass, 1).as_deref(), Some(BEER));
	}

	// --- until_next_minute ---

	#[test]
	fn sleeps_until_next_minute_boundary() {
		assert_eq!(
			until_next_minute(friday(9, 0, 0, 0)),
			Duration::from_secs(60)
		);
		assert_eq!(
			until_next_minute(friday(9, 0, 59, 500)),
			Duration::from_millis(500)
		);
		assert_eq!(
			until_next_minute(friday(9, 0, 15, 250)),
			Duration::from_millis(44_750)
		);
		// Leap second (59s + 1.5e9 ns): clamped rather than underflowing.
		let leap = friday(9, 0, 59, 0).with_nanosecond(1_500_000_000).unwrap();
		assert_eq!(until_next_minute(leap), Duration::from_nanos(1));
	}

	// --- stream test helpers ---

	/// Runs `test` on a tokio runtime on its own thread, failing if it takes
	/// more than 10 s of real time. That catches both a stream that never
	/// yields and one busy-looping inside a single poll, which no tokio
	/// timeout can interrupt.
	///
	/// With `paused`, tokio's clock starts paused and jumps ahead whenever the
	/// runtime is idle, so minute-long sleeps take no real time. Don't pause
	/// when the test waits on real IO (D-Bus): the clock would jump to the
	/// next timer while the IO is in flight.
	fn with_real_deadline(paused: bool, test: impl AsyncFnOnce() + Send + 'static) {
		let (done, result) = std::sync::mpsc::channel();
		std::thread::spawn(move || {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.start_paused(paused)
				.build()
				.unwrap();
			runtime.block_on(test());
			let _ = done.send(());
		});
		match result.recv_timeout(Duration::from_secs(10)) {
			Ok(()) => {}
			Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("test hung for 10 s"),
			Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
				panic!("test panicked, see its message above")
			}
		}
	}

	fn paused(test: impl AsyncFnOnce() + Send + 'static) {
		with_real_deadline(true, test)
	}

	fn real_time(test: impl AsyncFnOnce() + Send + 'static) {
		with_real_deadline(false, test)
	}

	/// A fake wall clock: `base` plus tokio's clock since now.
	fn fake_wall_clock(base: DateTime<Tz>) -> impl Fn() -> DateTime<Tz> {
		let start = tokio::time::Instant::now();
		move || base + TimeDelta::from_std(start.elapsed()).unwrap()
	}

	/// How long after the call each of the next `count` ticks arrives.
	async fn tick_times(ticks: impl Stream<Item = Message>, count: usize) -> Vec<Duration> {
		let start = tokio::time::Instant::now();
		let mut ticks = pin!(ticks);
		let mut times = Vec::new();
		for _ in 0..count {
			// With a paused clock a missing tick fails at once instead of
			// waiting for the real deadline.
			let tick = tokio::time::timeout(Duration::from_secs(3600), ticks.next()).await;
			assert_eq!(tick, Ok(Some(Message::Tick)), "no tick");
			times.push(start.elapsed());
		}
		times
	}

	// --- ticks ---

	#[test]
	fn ticks_land_on_minute_boundaries() {
		paused(async || {
			let clock = fake_wall_clock(friday(9, 0, 15, 250));
			let ticks = ticks(clock, futures_stream::pending());
			// 09:01:00, 09:02:00, 09:03:00.
			let expected = [44_750, 104_750, 164_750].map(Duration::from_millis);
			assert_eq!(tick_times(ticks, 3).await, expected);
		});
	}

	#[test]
	fn ticks_keep_landing_on_minute_boundaries_after_refreshes_end() {
		// Also guards against busy-looping on the ended `refreshes` stream.
		paused(async || {
			let clock = fake_wall_clock(friday(9, 0, 15, 250));
			let ticks = ticks(clock, futures_stream::empty());
			let expected = [44_750, 104_750].map(Duration::from_millis);
			assert_eq!(tick_times(ticks, 2).await, expected);
		});
	}

	#[test]
	fn wakeup_ticks_at_once_and_restarts_the_minute_phase() {
		paused(async || {
			// The wall clock also counts the time the test says was spent
			// suspended, which tokio's monotonic clock doesn't see.
			let awake_clock = fake_wall_clock(friday(13, 0, 30, 0));
			let suspended_ms = Arc::new(AtomicI64::new(0));
			let suspended = suspended_ms.clone();
			let clock =
				move || awake_clock() + TimeDelta::milliseconds(suspended.load(Ordering::SeqCst));
			let (wake, wakeups) = mpsc::unbounded::<()>();
			let start = tokio::time::Instant::now();
			let mut ticks = pin!(ticks(clock, wakeups));

			// 10 s into the ticker's 30 s sleep (wall clock 13:00:40)...
			let early = tokio::time::timeout(Duration::from_secs(10), ticks.next()).await;
			assert!(early.is_err(), "ticked before the minute boundary");
			// ...the machine is suspended for 20 min 5 s and wakes at 13:20:45.
			suspended_ms.store(20 * 60_000 + 5_000, Ordering::SeqCst);
			wake.unbounded_send(()).unwrap();

			assert_eq!(ticks.next().await, Some(Message::Tick));
			assert_eq!(start.elapsed(), Duration::from_secs(10), "wake-up tick");
			// Next tick at 13:21:00, i.e. 15 s later; not when the interrupted
			// sleep would have ended (20 s later, at 13:21:05).
			assert_eq!(ticks.next().await, Some(Message::Tick));
			assert_eq!(start.elapsed(), Duration::from_secs(25), "minute tick");
		});
	}

	// --- sleep_signals ---
	//
	// No timers here; `paused` only adds the real-time deadline.

	#[test]
	fn sleep_signals_passes_every_signal_on() {
		paused(async || {
			let signals = async { Ok(futures_stream::iter([(), (), ()])) };
			let signals: Vec<()> = sleep_signals(signals).collect().await;
			assert_eq!(signals.len(), 3);
		});
	}

	#[test]
	fn sleep_signals_ends_without_logind() {
		paused(async || {
			let signals = async {
				Err::<futures_stream::Empty<()>, _>(zbus::Error::Failure("no system bus".into()))
			};
			let signals: Vec<()> = sleep_signals(signals).collect().await;
			assert!(signals.is_empty());
		});
	}

	// --- logind over D-Bus ---

	/// The unique bus name the fake logind claims to own
	/// `org.freedesktop.login1` under.
	const FAKE_LOGIND_NAME: &str = ":1.1";

	/// A peer-to-peer D-Bus connection pair: (the applet's end, a fake
	/// logind's end), standing in for both the bus and logind. Before
	/// subscribing, zbus asks the bus (`GetNameOwner`) which unique name owns
	/// the proxy's service name, and then only accepts signals sent from that
	/// name. The fake answers `org.freedesktop.login1` with
	/// [`FAKE_LOGIND_NAME`] and every other method call with an error, so the
	/// applet only hears [`send_prepare_for_sleep`] if it asks for logind.
	async fn fake_logind() -> (zbus::Connection, zbus::Connection) {
		let (applet_end, logind_end) = tokio::net::UnixStream::pair().unwrap();
		let guid = zbus::Guid::generate();
		let (logind, applet) = future::join(
			zbus::connection::Builder::unix_stream(logind_end)
				.server(guid)
				.unwrap()
				.p2p()
				.build(),
			zbus::connection::Builder::unix_stream(applet_end)
				.p2p()
				.build(),
		)
		.await;
		let (applet, logind) = (applet.unwrap(), logind.unwrap());

		let replier = logind.clone();
		tokio::spawn(async move {
			let mut messages = zbus::MessageStream::from(&replier);
			while let Some(Ok(message)) = messages.next().await {
				if message.message_type() != zbus::message::Type::MethodCall {
					continue;
				}
				let header = message.header();
				let asks_for_logind = header.member().is_some_and(|m| m == "GetNameOwner")
					&& message.body().deserialize::<&str>().ok() == Some("org.freedesktop.login1");
				let _ = if asks_for_logind {
					replier.reply(&header, &FAKE_LOGIND_NAME).await
				} else {
					let error = "org.freedesktop.DBus.Error.NameHasNoOwner";
					replier.reply_error(&header, error, &()).await
				};
			}
		});
		(applet, logind)
	}

	/// Sends logind's `PrepareForSleep(start)` signal, from logind's unique
	/// name as the bus would.
	async fn send_prepare_for_sleep(logind: &zbus::Connection, start: bool) {
		let signal = zbus::Message::signal(
			"/org/freedesktop/login1",
			"org.freedesktop.login1.Manager",
			"PrepareForSleep",
		)
		.unwrap()
		.sender(FAKE_LOGIND_NAME)
		.unwrap()
		.build(&(start,))
		.unwrap();
		logind.send(&signal).await.unwrap();
	}

	#[test]
	fn logind_ticks_refresh_on_every_prepare_for_sleep() {
		// Real time: the D-Bus traffic is real IO. The fake clock stays at a
		// minute boundary, so the minute ticker would only tick 60 s from now
		// and every tick within the 10 s deadline comes from a signal.
		real_time(async || {
			let (applet, logind) = fake_logind().await;
			let at_boundary = move || friday(13, 0, 0, 0);
			let mut ticks = pin!(logind_ticks(at_boundary, async { Ok(applet) }));

			// The applet subscribes asynchronously, so keep waking up until it
			// has heard one.
			loop {
				send_prepare_for_sleep(&logind, false).await;
				let tick = tokio::time::timeout(Duration::from_millis(50), ticks.next()).await;
				if let Ok(tick) = tick {
					assert_eq!(tick, Some(Message::Tick));
					break;
				}
			}
			// Earlier wake-ups may have been heard too, after their 50 ms were
			// up; let those ticks through first.
			let quiet = Duration::from_millis(200);
			while tokio::time::timeout(quiet, ticks.next()).await.is_ok() {}
			// Subscribed now: going to sleep and waking up each tick once...
			for start in [true, false] {
				send_prepare_for_sleep(&logind, start).await;
				let tick = ticks.next().await;
				assert_eq!(tick, Some(Message::Tick), "start = {start}");
			}
			// ...and nothing else does.
			let tick = tokio::time::timeout(quiet, ticks.next()).await;
			assert!(tick.is_err(), "extra tick: {tick:?}");
		});
	}

	#[test]
	fn tick_stream_yields_a_minute_tick() {
		// Smoke test of the real wiring (system clock and system bus). It may
		// connect to the machine's logind or find no system bus (e.g. in the
		// Nix build sandbox); either way the first minute tick comes within
		// 60 s. Pausing the clock is fine although the bus is real IO: the
		// clock may jump ahead while the connection is set up, but this only
		// waits for the minute tick, not for anything from logind.
		paused(async || {
			let waited = tick_times(tick_stream(), 1).await[0];
			assert!(
				Duration::ZERO < waited && waited <= Duration::from_secs(60),
				"{waited:?}"
			);
		});
	}

	// --- AppModel ---

	fn app() -> AppModel {
		AppModel::init(Core::default(), ()).0
	}

	/// 2026-09-`day` at `hour`:00 in the machine's timezone. `AppModel` keeps
	/// `Local` times; `earliest` copes with a DST change at that time.
	fn local(day: u32, hour: u32) -> DateTime<Local> {
		let naive = NaiveDate::from_ymd_opt(2026, 9, day)
			.unwrap()
			.and_hms_opt(hour, 0, 0)
			.unwrap();
		Local.from_local_datetime(&naive).earliest().unwrap()
	}

	/// The texts `app.view()` shows, found by laying it out with a headless
	/// renderer and walking the widget tree.
	fn shown_texts(app: &AppModel) -> Vec<String> {
		use cosmic::iced::core::renderer::Headless;
		use cosmic::iced::core::widget::{Id, Operation};
		use cosmic::iced::core::{Font, Pixels, Rectangle, Size};
		use cosmic::iced::runtime::user_interface::{Cache, UserInterface};

		struct Texts(Vec<String>);
		impl Operation for Texts {
			fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
				operate(self);
			}
			fn text(&mut self, _id: Option<&Id>, _bounds: Rectangle, text: &str) {
				self.0.push(text.to_string());
			}
		}

		let new_renderer = <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), None);
		let mut renderer =
			cosmic::iced::futures::executor::block_on(new_renderer).expect("headless renderer");
		let size = Size::new(1000.0, 100.0);
		let mut ui = UserInterface::build(app.view(), size, Cache::default(), &mut renderer);
		let mut texts = Texts(Vec::new());
		ui.operate(&renderer, &mut texts);
		texts.0
	}

	#[test]
	fn starts_at_and_ticks_to_the_current_time() {
		let before = Local::now();
		let mut app = app();
		assert!(before <= app.now && app.now <= Local::now(), "after init");
		app.now = DateTime::UNIX_EPOCH.with_timezone(&Local);
		let before = Local::now();
		let _ = app.update(Message::Tick);
		assert!(before <= app.now && app.now <= Local::now(), "after a tick");
	}

	#[test]
	fn view_shows_the_label_for_the_current_time() {
		let mut app = app();
		// Friday 2026-09-11 (before and after `TARGET_HOUR`), when there is a
		// label...
		for hour in [0, 23] {
			app.now = local(11, hour);
			let label = label(app.now).expect("a label on Friday");
			assert_eq!(shown_texts(&app), [label], "{}", app.now);
		}
		// ...and the six days after it, when there is none.
		for day in 12..=17 {
			app.now = local(day, 12);
			assert_eq!(shown_texts(&app), [] as [String; 0], "{}", app.now);
		}
	}

	#[test]
	fn subscribes_to_the_tick_stream() {
		// Guards against subscribing to nothing (`Subscription::none()`) or to
		// another stream. iced tells subscriptions apart by their recipe's
		// hash, which for `Subscription::run` is the hash of the fn pointer.
		let hash = |subscription: Subscription<Message>| {
			let recipes = into_recipes(subscription);
			assert_eq!(recipes.len(), 1);
			let mut hasher = Hasher::default();
			recipes[0].hash(&mut hasher);
			std::hash::Hasher::finish(&hasher)
		};
		assert_eq!(
			hash(app().subscription()),
			hash(Subscription::run(tick_stream))
		);
	}

	// --- data files ---

	#[test]
	fn desktop_entry_and_icon_match_app_id() {
		let data = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
		let desktop = std::fs::read_to_string(format!("{data}/{APP_ID}.desktop")).unwrap();
		let lines: Vec<&str> = desktop.lines().collect();
		for expected in [
			format!("StartupWMClass={APP_ID}"),
			format!("Icon={APP_ID}-symbolic"),
			format!("Exec={}", env!("CARGO_PKG_NAME")),
			"X-CosmicApplet=true".to_string(),
		] {
			assert!(lines.contains(&expected.as_str()), "missing {expected}");
		}
		let icon = format!("{data}/icons/scalable/apps/{APP_ID}-symbolic.svg");
		assert!(std::path::Path::new(&icon).is_file(), "missing {icon}");
	}
}
