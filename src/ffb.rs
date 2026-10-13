//! Force feedback, driven by the game's own steering board object (clKickback).

use crate::*;
use sdl2::sys;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Default, serde::Deserialize, PartialEq, Copy, Clone)]
pub enum FfbMode {
	#[default]
	#[serde(rename = "haptic")]
	Haptic,
	#[serde(rename = "rumble")]
	Rumble,
	#[serde(rename = "board")]
	Board,
}

#[derive(serde::Deserialize)]
#[serde(default)]
pub struct FfbConfig {
	pub enabled: bool,
	pub mode: FfbMode,
	pub device: String,
	pub board_device: String,
	pub gain: u32,
	pub spring_gain: u32,
	pub damper_gain: u32,
	pub vibration_gain: u32,
	pub power_scale: u32,
	pub ignore_power_type: bool,
	pub reflect_range: u32,
	pub invert: bool,
	pub damper_floor: u32,
	pub spring_deadband: u32,
	pub require_motor_power: bool,
	pub rumble_threshold: u32,
	pub debug: bool,
}

impl Default for FfbConfig {
	fn default() -> Self {
		Self {
			enabled: false,
			mode: FfbMode::Haptic,
			device: String::new(),
			board_device: String::new(),
			gain: 100,
			spring_gain: 100,
			damper_gain: 100,
			vibration_gain: 100,
			power_scale: 50,
			ignore_power_type: false,
			reflect_range: 40,
			invert: false,
			damper_floor: 0,
			spring_deadband: 0,
			require_motor_power: true,
			rumble_threshold: 5,
			debug: false,
		}
	}
}

static mut CFG: Option<FfbConfig> = None;

fn cfg() -> &'static FfbConfig {
	unsafe { CFG.as_ref().unwrap() }
}

pub fn board_redirect(path: &str) -> Option<&'static str> {
	let c = unsafe { CFG.as_ref()? };
	if c.enabled && c.mode == FfbMode::Board && !c.board_device.is_empty() && path == "/dev/ttyM1" {
		Some(c.board_device.as_str())
	} else {
		None
	}
}

/// What the game last told the steering board.
#[derive(Clone, Copy, PartialEq)]
struct Board {
	spring: u8,
	viscosity: u8,
	reflection: i8,
	reflect_strength: f32,
	vib_strength: f32,
	vib_period: i32,
	vib_duration: i32,
	torque: bool,
	power_type: i32,
	motor: bool,
}

// Mirrors what clKickback::init leaves behind: torque on, power type 1.
static BOARD: Mutex<Board> = Mutex::new(Board {
	spring: 0,
	viscosity: 0,
	reflection: 0,
	reflect_strength: 0.0,
	vib_strength: 0.0,
	vib_period: 0,
	vib_duration: 0,
	torque: true,
	power_type: 1,
	motor: false,
});
static DIRTY: AtomicBool = AtomicBool::new(false);
static mut LAST_LOGGED: Option<Board> = None;

fn update(f: impl FnOnce(&mut Board)) {
	let snapshot = {
		let mut b = BOARD.lock().unwrap();
		f(&mut b);
		*b
	};
	DIRTY.store(true, Ordering::Release);
	if cfg().debug {
		unsafe {
			if LAST_LOGGED != Some(snapshot) {
				LAST_LOGGED = Some(snapshot);
				println!(
					"ffb: board spring={} damper={} reflect={} reflectEx={:.3} vib={:.3} period={} dur={} torque={} type={} motor={}",
					snapshot.spring, snapshot.viscosity, snapshot.reflection, snapshot.reflect_strength,
					snapshot.vib_strength, snapshot.vib_period, snapshot.vib_duration,
					snapshot.torque, snapshot.power_type, snapshot.motor
				);
			}
		}
	}
}

type ThisU8 = unsafe extern "C" fn(*const c_void, u8);
type ThisI8 = unsafe extern "C" fn(*const c_void, i8);
type ThisI32 = unsafe extern "C" fn(*const c_void, i32);
type ThisU16 = unsafe extern "C" fn(*const c_void, u16);
type ThisBool = unsafe extern "C" fn(*const c_void, bool);
type ThisF32I32 = unsafe extern "C" fn(*const c_void, f32, i32);
type ThisVibrate = unsafe extern "C" fn(*const c_void, f32, i32, i32);
type ThisCall = unsafe extern "C" fn(*const c_void) -> c_int;

static mut O_SPRING: Option<ThisU8> = None;
static mut O_CENTER: Option<ThisU16> = None;
static mut O_VISCOSITY: Option<ThisU8> = None;
static mut O_REFLECT: Option<ThisI8> = None;
static mut O_REFLECT_EX: Option<ThisF32I32> = None;
static mut O_CENTER_OFFSET: Option<ThisI32> = None;
static mut O_VIBRATE: Option<ThisVibrate> = None;
static mut O_ON_TRQ: Option<ThisBool> = None;
static mut O_OFF_TRQ: Option<ThisBool> = None;
static mut O_POWER_TYPE: Option<ThisI32> = None;
static mut O_ON_POWER: Option<ThisCall> = None;
static mut O_OFF_POWER: Option<ThisCall> = None;

unsafe extern "C" fn h_spring(this: *const c_void, v: u8) {
	O_SPRING.unwrap()(this, v);
	update(|b| b.spring = v);
}
unsafe extern "C" fn h_center(this: *const c_void, v: u16) {
	// Passed through only; centring is left to the device's spring effect.
	O_CENTER.unwrap()(this, v);
}
unsafe extern "C" fn h_viscosity(this: *const c_void, v: u8) {
	O_VISCOSITY.unwrap()(this, v);
	update(|b| b.viscosity = v);
}
unsafe extern "C" fn h_reflect(this: *const c_void, v: i8) {
	O_REFLECT.unwrap()(this, v);
	update(|b| b.reflection = v);
}
unsafe extern "C" fn h_reflect_ex(this: *const c_void, strength: f32, duration: i32) {
	O_REFLECT_EX.unwrap()(this, strength, duration);
	update(|b| {
		b.reflect_strength = strength;
		b.vib_duration = duration;
	});
}
unsafe extern "C" fn h_center_offset(this: *const c_void, v: i32) {
	O_CENTER_OFFSET.unwrap()(this, v);
}
unsafe extern "C" fn h_vibrate(this: *const c_void, strength: f32, period: i32, duration: i32) {
	O_VIBRATE.unwrap()(this, strength, period, duration);
	update(|b| {
		b.vib_strength = strength;
		b.vib_period = period;
		b.vib_duration = duration;
	});
}
unsafe extern "C" fn h_on_trq(this: *const c_void, immediate: bool) {
	O_ON_TRQ.unwrap()(this, immediate);
	update(|b| b.torque = true);
}
unsafe extern "C" fn h_off_trq(this: *const c_void, immediate: bool) {
	O_OFF_TRQ.unwrap()(this, immediate);
	update(|b| b.torque = false);
}
unsafe extern "C" fn h_power_type(this: *const c_void, t: i32) {
	O_POWER_TYPE.unwrap()(this, t);
	update(|b| b.power_type = t);
}
unsafe extern "C" fn h_on_power(this: *const c_void) -> c_int {
	let r = O_ON_POWER.unwrap()(this);
	update(|b| b.motor = true);
	r
}
unsafe extern "C" fn h_off_power(this: *const c_void) -> c_int {
	let r = O_OFF_POWER.unwrap()(this);
	update(|b| b.motor = false);
	r
}

macro_rules! hook_fn {
	($sym:expr, $detour:expr, $orig:ident) => {{
		let trampoline = hook::hook_symbol($sym, $detour as *const ());
		if trampoline.is_null() {
			println!("ffb: {} not found, not hooked", $sym);
			0
		} else {
			$orig = Some(transmute(trampoline));
			1
		}
	}};
}

pub unsafe fn init() {
	let parsed = match std::fs::read_to_string("ffb.toml") {
		Ok(text) => match toml::from_str::<FfbConfig>(&text) {
			Ok(c) => c,
			Err(e) => {
				eprintln!("ffb: ffb.toml is invalid, FFB disabled: {e}");
				FfbConfig::default()
			}
		},
		Err(_) => FfbConfig::default(),
	};
	CFG = Some(parsed);

	if cfg().enabled && cfg().mode == FfbMode::Board {
		// The game talks to the real board itself; nothing to hook.
		println!("ffb: original steering board mode, /dev/ttyM1 -> {:?}", cfg().board_device);
		return;
	}

	// debug alone is enough to see what the game sends, without driving a device
	if !cfg().enabled && !cfg().debug {
		return;
	}

	let mut n = 0;
	n += hook_fn!("_ZN10clKickback9setSpringEh", h_spring, O_SPRING);
	n += hook_fn!("_ZN10clKickback9setCenterEt", h_center, O_CENTER);
	n += hook_fn!("_ZN10clKickback10setViosityEh", h_viscosity, O_VISCOSITY);
	n += hook_fn!("_ZN10clKickback10setReflectEc", h_reflect, O_REFLECT);
	n += hook_fn!("_ZN10clKickback10setReflectEfi", h_reflect_ex, O_REFLECT_EX);
	n += hook_fn!("_ZN10clKickback15setCenterOffsetEi", h_center_offset, O_CENTER_OFFSET);
	n += hook_fn!("_ZN10clKickback10setVibrateEfii", h_vibrate, O_VIBRATE);
	n += hook_fn!("_ZN10clKickback5onTrqEb", h_on_trq, O_ON_TRQ);
	n += hook_fn!("_ZN10clKickback6offTrqEb", h_off_trq, O_OFF_TRQ);
	n += hook_fn!("_ZN10clKickback15setTRQPowerTypeEi", h_power_type, O_POWER_TYPE);
	n += hook_fn!("_ZN10clKickback7onPowerEv", h_on_power, O_ON_POWER);
	n += hook_fn!("_ZN10clKickback8offPowerEv", h_off_power, O_OFF_POWER);
	println!("ffb: hooked {n}/12 clKickback functions");

	if cfg().enabled {
		println!("ffb: enabled, mode = {:?}", cfg().mode);
	}
}

// SDL2 haptic #defines
const H_CONSTANT: u32 = 1 << 0;
const H_SINE: u32 = 1 << 1;
const H_SPRING: u32 = 1 << 7;
const H_DAMPER: u32 = 1 << 8;
const H_INFINITY: u32 = u32::MAX;
const H_STEERING_AXIS: u8 = 3;

struct Device {
	dev: *mut sys::SDL_Haptic,
	spring: c_int,
	damper: c_int,
	constant: c_int,
	sine: c_int,
	prev_vibration: f32,
}

unsafe fn sdl_error() -> String {
	CStr::from_ptr(sys::SDL_GetError()).to_string_lossy().to_string()
}

unsafe fn open_device() -> Option<Device> {
	if sys::SDL_InitSubSystem(sys::SDL_INIT_HAPTIC) != 0 {
		eprintln!("ffb: SDL haptic init failed: {}", sdl_error());
		return None;
	}
	let want = cfg().device.to_lowercase();
	let mut pick = None;
	for i in 0..sys::SDL_NumHaptics() {
		let name = CStr::from_ptr(sys::SDL_HapticName(i)).to_string_lossy().to_string();
		println!("ffb: haptic device {i}: {name}");
		if pick.is_none() && (want.is_empty() || name.to_lowercase().contains(&want)) {
			pick = Some(i);
		}
	}
	let Some(index) = pick else {
		eprintln!("ffb: no haptic device matches device = {:?}", cfg().device);
		return None;
	};
	let dev = sys::SDL_HapticOpen(index);
	if dev.is_null() {
		eprintln!("ffb: cannot open haptic device {index}: {}", sdl_error());
		return None;
	}
	let features = sys::SDL_HapticQuery(dev);
	println!("ffb: using device {index}, feature mask 0x{features:04X}");

	let mut d = Device {
		dev,
		spring: -1,
		damper: -1,
		constant: -1,
		sine: -1,
		prev_vibration: 0.0,
	};

	if features & (H_CONSTANT | H_SPRING | H_DAMPER) == 0 {
		eprintln!("ffb: device has no wheel effects");
		sys::SDL_HapticClose(dev);
		return None;
	}
	// Create each supported effect once at zero strength, then only update it.
	if features & H_SPRING != 0 {
		d.spring = new_condition(&d, H_SPRING as u16);
	}
	if features & H_DAMPER != 0 {
		d.damper = new_condition(&d, H_DAMPER as u16);
	}
	if features & H_CONSTANT != 0 {
		let mut e: sys::SDL_HapticEffect = std::mem::zeroed();
		e.constant.type_ = H_CONSTANT as u16;
		e.constant.direction.type_ = H_STEERING_AXIS;
		e.constant.length = H_INFINITY;
		d.constant = new_run(&d, &mut e);
	}
	if features & H_SINE != 0 {
		let mut e: sys::SDL_HapticEffect = std::mem::zeroed();
		e.periodic.type_ = H_SINE as u16;
		e.periodic.direction.type_ = H_STEERING_AXIS;
		e.periodic.length = H_INFINITY;
		e.periodic.period = 20;
		d.sine = new_run(&d, &mut e);
	}
	Some(d)
}

unsafe fn new_run(d: &Device, e: &mut sys::SDL_HapticEffect) -> c_int {
	let id = sys::SDL_HapticNewEffect(d.dev, e);
	if id < 0 {
		eprintln!("ffb: cannot create effect: {}", sdl_error());
		return -1;
	}
	if sys::SDL_HapticRunEffect(d.dev, id, 1) < 0 {
		eprintln!("ffb: cannot run effect: {}", sdl_error());
	}
	id
}

unsafe fn new_condition(d: &Device, kind: u16) -> c_int {
	let mut e: sys::SDL_HapticEffect = std::mem::zeroed();
	e.condition.type_ = kind;
	e.condition.direction.type_ = H_STEERING_AXIS;
	e.condition.length = H_INFINITY;
	e.condition.right_sat[0] = 65535;
	e.condition.left_sat[0] = 65535;
	new_run(d, &mut e)
}

unsafe fn set_condition(d: &Device, id: c_int, kind: u16, strength: f32, deadband: f32) {
	if id < 0 {
		return;
	}
	let mut e: sys::SDL_HapticEffect = std::mem::zeroed();
	e.condition.type_ = kind;
	e.condition.direction.type_ = H_STEERING_AXIS;
	e.condition.length = H_INFINITY;
	e.condition.right_sat[0] = 65535;
	e.condition.left_sat[0] = 65535;
	e.condition.deadband[0] = (deadband.clamp(0.0, 1.0) * 65535.0) as u16;
	let c = (strength.clamp(0.0, 1.0) * 32767.0) as i16;
	e.condition.right_coeff[0] = c;
	e.condition.left_coeff[0] = c;
	if sys::SDL_HapticUpdateEffect(d.dev, id, &mut e) < 0 {
		eprintln!("ffb: condition update failed: {}", sdl_error());
	}
}

/// Forces the board asks for, all 0..1 except `constant` (-1..1).
struct Forces {
	spring: f32,
	damper: f32,
	constant: f32,
	vibration: f32,
}

fn compute(b: &Board) -> Forces {
	let c = cfg();
	let pct = |v: u32| v as f32 / 100.0;
	let power_on = b.power_type != 0 || c.ignore_power_type;
	let scale = if power_on { pct(c.power_scale) } else { 0.0 };
	let gain = pct(c.gain);

	let range = if c.reflect_range > 0 { c.reflect_range as f32 } else { 127.0 };
	let mut reflect = if b.reflection != 0 {
		b.reflection as f32 / range
	} else {
		b.reflect_strength
	};
	if c.invert {
		reflect = -reflect;
	}

	let enabled = b.torque && power_on && (b.motor || !c.require_motor_power);
	if !enabled {
		return Forces { spring: 0.0, damper: 0.0, constant: 0.0, vibration: 0.0 };
	}

	let mut damper = b.viscosity as f32 / 127.0 * scale * gain * pct(c.damper_gain);
	damper = damper.max(pct(c.damper_floor));
	Forces {
		spring: b.spring as f32 / 127.0 * scale * gain * pct(c.spring_gain),
		damper,
		constant: (reflect * scale * gain).clamp(-1.0, 1.0),
		vibration: (b.vib_strength.abs() * scale * gain * pct(c.vibration_gain)).clamp(0.0, 1.0),
	}
}

unsafe fn apply_haptic(d: &mut Device, f: &Forces, b: &Board) {
	set_condition(d, d.spring, H_SPRING as u16, f.spring, pct_f(cfg().spring_deadband));
	set_condition(d, d.damper, H_DAMPER as u16, f.damper, 0.0);

	if d.constant >= 0 {
		let mut e: sys::SDL_HapticEffect = std::mem::zeroed();
		e.constant.type_ = H_CONSTANT as u16;
		e.constant.direction.type_ = H_STEERING_AXIS;
		e.constant.length = H_INFINITY;
		e.constant.level = (f.constant * 32767.0) as i16;
		if sys::SDL_HapticUpdateEffect(d.dev, d.constant, &mut e) < 0 {
			eprintln!("ffb: constant update failed: {}", sdl_error());
		}
	}

	if d.sine >= 0 {
		let mut e: sys::SDL_HapticEffect = std::mem::zeroed();
		e.periodic.type_ = H_SINE as u16;
		e.periodic.direction.type_ = H_STEERING_AXIS;
		e.periodic.length = if b.vib_duration > 0 { b.vib_duration as u32 } else { H_INFINITY };
		e.periodic.period = if b.vib_period > 0 { b.vib_period as u16 } else { 20 };
		e.periodic.magnitude = (f.vibration * 32767.0) as i16;
		if sys::SDL_HapticUpdateEffect(d.dev, d.sine, &mut e) < 0 {
			eprintln!("ffb: vibration update failed: {}", sdl_error());
		}
		// A new burst restarts the effect, because a finite one has ended by now.
		if f.vibration > 0.001 && d.prev_vibration <= 0.001 {
			sys::SDL_HapticRunEffect(d.dev, d.sine, 1);
		}
		d.prev_vibration = f.vibration;
	}
}

fn pct_f(v: u32) -> f32 {
	v as f32 / 100.0
}

enum Out {
	Haptic(Device),
	Rumble,
}

struct Runtime {
	out: Option<Out>,
	tried: bool,
	rumbling: bool,
	last_rumble: Instant,
	last_strength: f32,
}

static mut RT: Option<Runtime> = None;

unsafe fn pick_output() -> Option<Out> {
	if cfg().mode == FfbMode::Rumble {
		println!("ffb: using gamepad rumble");
		return Some(Out::Rumble);
	}
	match open_device() {
		Some(d) => Some(Out::Haptic(d)),
		None => {
			println!("ffb: no usable wheel device, falling back to gamepad rumble");
			Some(Out::Rumble)
		}
	}
}

/// Called once per frame from the input handler, after SDL is initialised.
pub unsafe fn tick(sdl: &mut poll::PollState) {
	if CFG.is_none() || !cfg().enabled || cfg().mode == FfbMode::Board {
		return;
	}
	let rt = RT.get_or_insert_with(|| Runtime {
		out: None,
		tried: false,
		rumbling: false,
		last_rumble: Instant::now(),
		last_strength: 0.0,
	});
	if !rt.tried {
		rt.tried = true;
		println!("ffb: controllers: {:?}", sdl.controller_names());
		rt.out = pick_output();
	}

	let dirty = DIRTY.swap(false, Ordering::AcqRel);
	let board = *BOARD.lock().unwrap();
	let forces = compute(&board);

	if matches!(rt.out, Some(Out::Rumble)) {
		rumble_tick(rt, sdl, &forces);
	} else if let Some(Out::Haptic(d)) = rt.out.as_mut() {
		if dirty {
			apply_haptic(d, &forces, &board);
		}
	}
}

/// Heavy motor = steady/impact force (reflection, e.g. a wall hit), light motor = vibration.
unsafe fn rumble_tick(rt: &mut Runtime, sdl: &mut poll::PollState, f: &Forces) {
	let th = pct_f(cfg().rumble_threshold);
	let low = if f.constant.abs() > th { f.constant.abs() } else { 0.0 };
	let high = if f.vibration > th { f.vibration } else { 0.0 };
	let strength = low.max(high);

	if strength > 0.0 {
		// Re-issued every ~50 ms with a 120 ms duration, so it stops by itself
		// if the game stops asking.
		let due = rt.last_rumble.elapsed() >= Duration::from_millis(50);
		if due || (strength - rt.last_strength).abs() > 0.05 {
			let n = sdl.rumble((low * 65535.0) as u16, (high * 65535.0) as u16, 120);
			if !rt.rumbling && cfg().debug {
				println!("ffb: rumble start low={low:.2} high={high:.2}, {n} controller(s) accepted it");
			}
			rt.last_rumble = Instant::now();
			rt.last_strength = strength;
			rt.rumbling = true;
		}
	} else if rt.rumbling {
		sdl.rumble(0, 0, 0);
		rt.rumbling = false;
		rt.last_strength = 0.0;
	}
}