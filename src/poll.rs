use device_query::DeviceQuery;
use phf::*;
use sdl2::controller::Button;
use sdl2::event::Event;
use sdl2::keyboard::Keycode as SdlKeycode;
use sdl2::*;
use std::collections::*;

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Axis {
	LeftStickLeft,
	LeftStickUp,
	LeftStickDown,
	LeftStickRight,
	RightStickLeft,
	RightStickUp,
	RightStickDown,
	RightStickRight,
	LeftTriggerDown,
	LeftTriggerUp,
	RightTriggerDown,
	RightTriggerUp,
}

#[allow(dead_code)]
pub struct PollState {
	sdl: Sdl,
	video: VideoSubsystem,
	gamepad: GameControllerSubsystem,
	joystick: JoystickSubsystem,
	events: EventPump,
	controllers: BTreeMap<u32, controller::GameController>,
	window: *mut sdl2::sys::SDL_Window,
	deadzone: f32,
	keyboard_state: HashSet<SdlKeycode>,
	last_keyboard_state: HashSet<SdlKeycode>,
	button_state: Vec<Button>,
	last_button_state: Vec<Button>,
	axis_state: BTreeMap<Axis, f32>,
	last_axis_state: BTreeMap<Axis, f32>,
	window_handle: *const libc::c_void,
	keyboard_axis_values: BTreeMap<String, f32>,
}

#[derive(Clone)]
pub enum KeyBinding {
	Keycode(SdlKeycode),
	Button(Button),
	Axis(Axis),
}

pub struct KeyBindings {
	keys: Vec<KeyBinding>,
}

const MAPPINGS: Map<&str, KeyBinding> = phf_map! {
	"F1" => KeyBinding::Keycode(SdlKeycode::F1),
	"F2" => KeyBinding::Keycode(SdlKeycode::F2),
	"F3" => KeyBinding::Keycode(SdlKeycode::F3),
	"F4" => KeyBinding::Keycode(SdlKeycode::F4),
	"F5" => KeyBinding::Keycode(SdlKeycode::F5),
	"F6" => KeyBinding::Keycode(SdlKeycode::F6),
	"F7" => KeyBinding::Keycode(SdlKeycode::F7),
	"F8" => KeyBinding::Keycode(SdlKeycode::F8),
	"F9" => KeyBinding::Keycode(SdlKeycode::F9),
	"F10" => KeyBinding::Keycode(SdlKeycode::F10),
	"F11" => KeyBinding::Keycode(SdlKeycode::F11),
	"F12" => KeyBinding::Keycode(SdlKeycode::F12),
	"NUM0" => KeyBinding::Keycode(SdlKeycode::Num0),
	"NUM1" => KeyBinding::Keycode(SdlKeycode::Num1),
	"NUM2" => KeyBinding::Keycode(SdlKeycode::Num2),
	"NUM3" => KeyBinding::Keycode(SdlKeycode::Num3),
	"NUM4" => KeyBinding::Keycode(SdlKeycode::Num4),
	"NUM5" => KeyBinding::Keycode(SdlKeycode::Num5),
	"NUM6" => KeyBinding::Keycode(SdlKeycode::Num6),
	"NUM7" => KeyBinding::Keycode(SdlKeycode::Num7),
	"NUM8" => KeyBinding::Keycode(SdlKeycode::Num8),
	"NUM9" => KeyBinding::Keycode(SdlKeycode::Num9),
	"UPARROW" => KeyBinding::Keycode(SdlKeycode::Up),
	"LEFTARROW" => KeyBinding::Keycode(SdlKeycode::Left),
	"DOWNARROW" => KeyBinding::Keycode(SdlKeycode::Down),
	"RIGHTARROW" => KeyBinding::Keycode(SdlKeycode::Right),
	"ENTER" => KeyBinding::Keycode(SdlKeycode::Return),
	"SPACE" => KeyBinding::Keycode(SdlKeycode::Space),
	"CONTROL" => KeyBinding::Keycode(SdlKeycode::LCtrl),
	"SHIFT" => KeyBinding::Keycode(SdlKeycode::LShift),
	"TAB" => KeyBinding::Keycode(SdlKeycode::Tab),
	"ESCAPE" => KeyBinding::Keycode(SdlKeycode::Escape),
	"A" => KeyBinding::Keycode(SdlKeycode::A),
	"B" => KeyBinding::Keycode(SdlKeycode::B),
	"C" => KeyBinding::Keycode(SdlKeycode::C),
	"D" => KeyBinding::Keycode(SdlKeycode::D),
	"E" => KeyBinding::Keycode(SdlKeycode::E),
	"F" => KeyBinding::Keycode(SdlKeycode::F),
	"G" => KeyBinding::Keycode(SdlKeycode::G),
	"H" => KeyBinding::Keycode(SdlKeycode::H),
	"I" => KeyBinding::Keycode(SdlKeycode::I),
	"J" => KeyBinding::Keycode(SdlKeycode::J),
	"K" => KeyBinding::Keycode(SdlKeycode::K),
	"L" => KeyBinding::Keycode(SdlKeycode::L),
	"M" => KeyBinding::Keycode(SdlKeycode::M),
	"N" => KeyBinding::Keycode(SdlKeycode::N),
	"O" => KeyBinding::Keycode(SdlKeycode::O),
	"P" => KeyBinding::Keycode(SdlKeycode::P),
	"Q" => KeyBinding::Keycode(SdlKeycode::Q),
	"R" => KeyBinding::Keycode(SdlKeycode::R),
	"S" => KeyBinding::Keycode(SdlKeycode::S),
	"T" => KeyBinding::Keycode(SdlKeycode::T),
	"U" => KeyBinding::Keycode(SdlKeycode::U),
	"V" => KeyBinding::Keycode(SdlKeycode::V),
	"W" => KeyBinding::Keycode(SdlKeycode::W),
	"X" => KeyBinding::Keycode(SdlKeycode::X),
	"Y" => KeyBinding::Keycode(SdlKeycode::Y),
	"Z" => KeyBinding::Keycode(SdlKeycode::Z),
	"SDL_A" => KeyBinding::Button(Button::A),
	"SDL_B" => KeyBinding::Button(Button::B),
	"SDL_X" => KeyBinding::Button(Button::X),
	"SDL_Y" => KeyBinding::Button(Button::Y),
	"SDL_BACK" => KeyBinding::Button(Button::Back),
	"SDL_GUIDE" => KeyBinding::Button(Button::Guide),
	"SDL_START" => KeyBinding::Button(Button::Start),
	"SDL_LSHOULDER" => KeyBinding::Button(Button::LeftShoulder),
	"SDL_RSHOULDER" => KeyBinding::Button(Button::RightShoulder),
	"SDL_DPAD_UP" => KeyBinding::Button(Button::DPadUp),
	"SDL_DPAD_LEFT" => KeyBinding::Button(Button::DPadLeft),
	"SDL_DPAD_DOWN" => KeyBinding::Button(Button::DPadDown),
	"SDL_DPAD_RIGHT" => KeyBinding::Button(Button::DPadRight),
	"SDL_MISC" => KeyBinding::Button(Button::Misc1),
	"SDL_PADDLE1" => KeyBinding::Button(Button::Paddle1),
	"SDL_PADDLE2" => KeyBinding::Button(Button::Paddle2),
	"SDL_PADDLE3" => KeyBinding::Button(Button::Paddle3),
	"SDL_PADDLE4" => KeyBinding::Button(Button::Paddle4),
	"SDL_TOUCHPAD" => KeyBinding::Button(Button::Touchpad),
	"SDL_LSTICK_PRESS" => KeyBinding::Button(Button::LeftStick),
	"SDL_RSTICK_PRESS" => KeyBinding::Button(Button::RightStick),
	"SDL_LSTICK_LEFT" => KeyBinding::Axis(Axis::LeftStickLeft),
	"SDL_LSTICK_UP" => KeyBinding::Axis(Axis::LeftStickUp),
	"SDL_LSTICK_DOWN" => KeyBinding::Axis(Axis::LeftStickDown),
	"SDL_LSTICK_RIGHT" => KeyBinding::Axis(Axis::LeftStickRight),
	"SDL_RSTICK_LEFT" => KeyBinding::Axis(Axis::RightStickLeft),
	"SDL_RSTICK_UP" => KeyBinding::Axis(Axis::RightStickUp),
	"SDL_RSTICK_DOWN" => KeyBinding::Axis(Axis::RightStickDown),
	"SDL_RSTICK_RIGHT" => KeyBinding::Axis(Axis::RightStickRight),
	"SDL_LTRIGGER_DOWN" => KeyBinding::Axis(Axis::LeftTriggerDown),
	"SDL_LTRIGGER_UP" => KeyBinding::Axis(Axis::LeftTriggerUp),
	"SDL_RTRIGGER_DOWN" => KeyBinding::Axis(Axis::RightTriggerDown),
	"SDL_RTRIGGER_UP" => KeyBinding::Axis(Axis::RightTriggerUp),
};

pub fn parse_keybinding(toml: Vec<String>) -> KeyBindings {
	let mut keybindings = KeyBindings { keys: Vec::new() };
	for value in toml {
		if let Some(keybinding) = MAPPINGS.get(&value) {
			keybindings.keys.push(keybinding.clone());
		} else {
			panic!("Incorrect keybinding {value}");
		}
	}
	keybindings
}

impl PollState {
	pub fn new(handle: *const libc::c_void, axis_deadzone: f32) -> Result<Self, String> {
		let sdl = sdl2::init()?;
		let video = sdl.video()?;
		let joystick = sdl.joystick()?;
		let gamepad = sdl.game_controller()?;
		let events = sdl.event_pump()?;

		let _ = gamepad.load_mappings("gamecontrollerdb.txt");
		let mut controllers = BTreeMap::new();
		for i in 0..gamepad.num_joysticks()? {
			if let Ok(controller) = gamepad.open(i) {
				controllers.insert(i, controller);
			}
		}

		Ok(Self {
			sdl,
			video,
			gamepad,
			joystick,
			events,
			controllers,
			window: std::ptr::null_mut(),
			deadzone: axis_deadzone,
			keyboard_state: HashSet::new(),
			last_keyboard_state: HashSet::new(),
			button_state: Vec::with_capacity(32),
			last_button_state: Vec::with_capacity(32),
			axis_state: BTreeMap::new(),
			last_axis_state: BTreeMap::new(),
			window_handle: handle,
			keyboard_axis_values: BTreeMap::new(),
		})
	}

	pub fn update(&mut self) {
		self.last_keyboard_state = self.keyboard_state.clone();
		self.last_button_state.clear();
		self.last_axis_state.clear();

		self.last_button_state.extend(&self.button_state);
		self.last_axis_state.extend(&self.axis_state);

		// Only poll keyboard if the game window is currently in focus
		if unsafe { crate::adm::WINDOW_FOCUSED } {
			let device_keys = device_query::DeviceState::new().get_keys();
			self.keyboard_state.clear();
			for k in device_keys {
				let keycode_opt = match k {
					device_query::Keycode::F1 => Some(SdlKeycode::F1),
					device_query::Keycode::F2 => Some(SdlKeycode::F2),
					device_query::Keycode::F3 => Some(SdlKeycode::F3),
					device_query::Keycode::F4 => Some(SdlKeycode::F4),
					device_query::Keycode::F5 => Some(SdlKeycode::F5),
					device_query::Keycode::F6 => Some(SdlKeycode::F6),
					device_query::Keycode::F7 => Some(SdlKeycode::F7),
					device_query::Keycode::F8 => Some(SdlKeycode::F8),
					device_query::Keycode::F9 => Some(SdlKeycode::F9),
					device_query::Keycode::F10 => Some(SdlKeycode::F10),
					device_query::Keycode::F11 => Some(SdlKeycode::F11),
					device_query::Keycode::F12 => Some(SdlKeycode::F12),
					device_query::Keycode::Key0 => Some(SdlKeycode::Num0),
					device_query::Keycode::Key1 => Some(SdlKeycode::Num1),
					device_query::Keycode::Key2 => Some(SdlKeycode::Num2),
					device_query::Keycode::Key3 => Some(SdlKeycode::Num3),
					device_query::Keycode::Key4 => Some(SdlKeycode::Num4),
					device_query::Keycode::Key5 => Some(SdlKeycode::Num5),
					device_query::Keycode::Key6 => Some(SdlKeycode::Num6),
					device_query::Keycode::Key7 => Some(SdlKeycode::Num7),
					device_query::Keycode::Key8 => Some(SdlKeycode::Num8),
					device_query::Keycode::Key9 => Some(SdlKeycode::Num9),
					device_query::Keycode::Up => Some(SdlKeycode::Up),
					device_query::Keycode::Left => Some(SdlKeycode::Left),
					device_query::Keycode::Down => Some(SdlKeycode::Down),
					device_query::Keycode::Right => Some(SdlKeycode::Right),
					device_query::Keycode::Enter => Some(SdlKeycode::Return),
					device_query::Keycode::Space => Some(SdlKeycode::Space),
					device_query::Keycode::LControl => Some(SdlKeycode::LCtrl),
					device_query::Keycode::LShift => Some(SdlKeycode::LShift),
					device_query::Keycode::Escape => Some(SdlKeycode::Escape),
					device_query::Keycode::Tab => Some(SdlKeycode::Tab),
					device_query::Keycode::A => Some(SdlKeycode::A),
					device_query::Keycode::B => Some(SdlKeycode::B),
					device_query::Keycode::C => Some(SdlKeycode::C),
					device_query::Keycode::D => Some(SdlKeycode::D),
					device_query::Keycode::E => Some(SdlKeycode::E),
					device_query::Keycode::F => Some(SdlKeycode::F),
					device_query::Keycode::G => Some(SdlKeycode::G),
					device_query::Keycode::H => Some(SdlKeycode::H),
					device_query::Keycode::I => Some(SdlKeycode::I),
					device_query::Keycode::J => Some(SdlKeycode::J),
					device_query::Keycode::K => Some(SdlKeycode::K),
					device_query::Keycode::L => Some(SdlKeycode::L),
					device_query::Keycode::M => Some(SdlKeycode::M),
					device_query::Keycode::N => Some(SdlKeycode::N),
					device_query::Keycode::O => Some(SdlKeycode::O),
					device_query::Keycode::P => Some(SdlKeycode::P),
					device_query::Keycode::Q => Some(SdlKeycode::Q),
					device_query::Keycode::R => Some(SdlKeycode::R),
					device_query::Keycode::S => Some(SdlKeycode::S),
					device_query::Keycode::T => Some(SdlKeycode::T),
					device_query::Keycode::U => Some(SdlKeycode::U),
					device_query::Keycode::V => Some(SdlKeycode::V),
					device_query::Keycode::W => Some(SdlKeycode::W),
					device_query::Keycode::X => Some(SdlKeycode::X),
					device_query::Keycode::Y => Some(SdlKeycode::Y),
					device_query::Keycode::Z => Some(SdlKeycode::Z),
					_ => None,
				};
				if let Some(keycode) = keycode_opt {
					self.keyboard_state.insert(keycode);
				}
			}
		} else {
			self.keyboard_state.clear();
		}

		// Smooth analog ramping for keyboard controls using separate sensitivities
		let steering_sens = unsafe { crate::CONFIG.keyboard_steering_sensitivity };
		let pedal_sens = unsafe { crate::CONFIG.keyboard_pedal_sensitivity };
		let dt = 0.016;

		let mut gas_pressed = false;
		let mut brake_pressed = false;
		let mut left_pressed = false;
		let mut right_pressed = false;

		if let Some(ref kc) = unsafe { crate::KEYCONFIG.as_ref() } {
			gas_pressed = kc.gas.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if self.keycode_is_down(k)));
			brake_pressed = kc.brake.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if self.keycode_is_down(k)));
			left_pressed = kc.wheel_left.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if self.keycode_is_down(k)));
			right_pressed = kc.wheel_right.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if self.keycode_is_down(k)));
		}

		let mut update_axis = |name: &str, is_pressed: bool, sensitivity: f32| {
			let val = self.keyboard_axis_values.entry(name.to_string()).or_insert(0.0);
			if is_pressed {
				*val = (*val + sensitivity * dt).min(1.0);
			} else {
				*val = (*val - sensitivity * dt).max(0.0);
			}
		};

		update_axis("gas", gas_pressed, pedal_sens);
		update_axis("brake", brake_pressed, pedal_sens);
		update_axis("wheel_left", left_pressed, steering_sens);
		update_axis("wheel_right", right_pressed, steering_sens);

		for event in self.events.poll_iter() {
			match event {
				Event::ControllerDeviceAdded { which, .. } => {
					if let Ok(controller) = self.gamepad.open(which) {
						self.controllers.insert(which, controller);
					}
				}
				Event::ControllerDeviceRemoved { which, .. } => {
					if let Some(controller) = self.controllers.remove(&which) {
						drop(controller);
					}
				}
				Event::ControllerButtonDown { button, .. } => {
					self.button_state.push(button);
				}
				Event::ControllerButtonUp { button, .. } => {
					self.button_state.retain(|b| b != &button);
				}
				Event::ControllerAxisMotion { axis, value, .. } => {
					let value = value as f32 / i16::MAX as f32;
					use Axis::*;
					let (axis_positive, axis_negative) = match axis {
						controller::Axis::LeftX => (LeftStickRight, LeftStickLeft),
						controller::Axis::LeftY => (LeftStickDown, LeftStickUp),
						controller::Axis::RightX => (RightStickRight, RightStickLeft),
						controller::Axis::RightY => (RightStickDown, RightStickUp),
						controller::Axis::TriggerLeft => (LeftTriggerDown, LeftTriggerUp),
						controller::Axis::TriggerRight => (RightTriggerDown, RightTriggerUp),
					};
					if value > self.deadzone {
						self.axis_state.insert(axis_positive, value);
						self.axis_state.insert(axis_negative, 0.0);
					} else if value < -self.deadzone {
						self.axis_state.insert(axis_negative, -value);
						self.axis_state.insert(axis_positive, 0.0);
					} else {
						self.axis_state.insert(axis_positive, 0.0);
						self.axis_state.insert(axis_negative, 0.0);
					}
				}
				_ => {}
			}
		}
	}

	fn keycode_is_down(&self, keycode: &SdlKeycode) -> bool {
		self.keyboard_state.contains(keycode)
	}
	fn keycode_is_up(&self, keycode: &SdlKeycode) -> bool {
		!self.keyboard_state.contains(keycode)
	}
	fn keycode_was_down(&self, keycode: &SdlKeycode) -> bool {
		self.last_keyboard_state.contains(keycode)
	}
	fn keycode_was_up(&self, keycode: &SdlKeycode) -> bool {
		!self.last_keyboard_state.contains(keycode)
	}
	fn keycode_is_tapped(&self, keycode: &SdlKeycode) -> bool {
		self.keycode_is_down(keycode) && self.keycode_was_up(keycode)
	}
	fn keycode_is_released(&self, keycode: &SdlKeycode) -> bool {
		self.keycode_is_up(keycode) && self.keycode_was_down(keycode)
	}

	fn button_is_down(&self, button: &Button) -> bool {
		self.button_state.contains(button)
	}
	fn button_is_up(&self, button: &Button) -> bool {
		!self.button_state.contains(button)
	}
	fn button_was_down(&self, button: &Button) -> bool {
		self.last_button_state.contains(button)
	}
	fn button_was_up(&self, button: &Button) -> bool {
		!self.last_button_state.contains(button)
	}
	fn button_is_tapped(&self, button: &Button) -> bool {
		self.button_is_down(button) && self.button_was_up(button)
	}
	fn button_is_released(&self, button: &Button) -> bool {
		self.button_was_down(button) && self.button_is_up(button)
	}

	fn axis_is_down(&self, axis: &Axis) -> f32 {
		*self.axis_state.get(axis).unwrap_or(&0.0)
	}
	fn axis_is_up(&self, axis: &Axis) -> bool {
		self.axis_is_down(axis) == 0.0
	}
	fn axis_was_down(&self, axis: &Axis) -> f32 {
		*self.last_axis_state.get(axis).unwrap_or(&0.0)
	}
	fn axis_was_up(&self, axis: &Axis) -> bool {
		self.axis_was_down(axis) == 0.0
	}
	fn axis_is_tapped(&self, axis: &Axis) -> bool {
		self.axis_is_down(axis) != 0.0 && self.axis_was_up(axis)
	}
	fn axis_is_released(&self, axis: &Axis) -> bool {
		self.axis_was_down(axis) != 0.0 && self.axis_is_up(axis)
	}

	fn binding_is_down(&self, keybinding: &KeyBinding) -> f32 {
		match keybinding {
			KeyBinding::Keycode(keycode) => {
				if let Some(ref kc) = unsafe { crate::KEYCONFIG.as_ref() } {
					if kc.gas.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if k == keycode)) {
						return *self.keyboard_axis_values.get("gas").unwrap_or(&0.0);
					}
					if kc.brake.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if k == keycode)) {
						return *self.keyboard_axis_values.get("brake").unwrap_or(&0.0);
					}
					if kc.wheel_left.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if k == keycode)) {
						return *self.keyboard_axis_values.get("wheel_left").unwrap_or(&0.0);
					}
					if kc.wheel_right.keys.iter().any(|b| matches!(b, KeyBinding::Keycode(k) if k == keycode)) {
						return *self.keyboard_axis_values.get("wheel_right").unwrap_or(&0.0);
					}
				}
				self.keycode_is_down(keycode) as i32 as f32
			}
			KeyBinding::Button(button) => self.button_is_down(button) as i32 as f32,
			KeyBinding::Axis(axis) => self.axis_is_down(axis),
		}
	}
	fn binding_is_up(&self, keybinding: &KeyBinding) -> bool {
		match keybinding {
			KeyBinding::Keycode(keycode) => self.keycode_is_up(keycode),
			KeyBinding::Button(button) => self.button_is_up(button),
			KeyBinding::Axis(axis) => self.axis_is_up(axis),
		}
	}
	fn binding_was_down(&self, keybinding: &KeyBinding) -> f32 {
		match keybinding {
			KeyBinding::Keycode(keycode) => self.keycode_was_down(keycode) as i32 as f32,
			KeyBinding::Button(button) => self.button_was_down(button) as i32 as f32,
			KeyBinding::Axis(axis) => self.axis_was_down(axis),
		}
	}
	fn binding_was_up(&self, keybinding: &KeyBinding) -> bool {
		match keybinding {
			KeyBinding::Keycode(keycode) => self.keycode_was_up(keycode),
			KeyBinding::Button(button) => self.button_was_up(button),
			KeyBinding::Axis(axis) => self.axis_was_up(axis),
		}
	}
	fn binding_is_tapped(&self, keybinding: &KeyBinding) -> bool {
		match keybinding {
			KeyBinding::Keycode(keycode) => self.keycode_is_tapped(keycode),
			KeyBinding::Button(button) => self.button_is_tapped(button),
			KeyBinding::Axis(axis) => self.axis_is_tapped(axis),
		}
	}
	fn binding_is_released(&self, keybinding: &KeyBinding) -> bool {
		match keybinding {
			KeyBinding::Keycode(keycode) => self.keycode_is_released(keycode),
			KeyBinding::Button(button) => self.button_is_released(button),
			KeyBinding::Axis(axis) => self.axis_is_released(axis),
		}
	}

	pub fn is_down(&self, keybindings: &KeyBindings) -> f32 {
		for keybinding in keybindings.keys.iter() {
			let value = self.binding_is_down(keybinding);
			if value > 0.0 {
				return value;
			}
		}
		0.0
	}
	pub fn is_up(&self, keybindings: &KeyBindings) -> bool {
		for keybinding in keybindings.keys.iter() {
			if self.binding_is_up(keybinding) {
				return true;
			}
		}
		false
	}
	pub fn was_down(&self, keybindings: &KeyBindings) -> f32 {
		for keybinding in keybindings.keys.iter() {
			let value = self.binding_was_down(keybinding);
			if value > 0.0 {
				return value;
			}
		}
		0.0
	}
	pub fn was_up(&self, keybindings: &KeyBindings) -> bool {
		for keybinding in keybindings.keys.iter() {
			if self.binding_was_up(keybinding) {
				return true;
			}
		}
		false
	}
	pub fn is_tapped(&self, keybindings: &KeyBindings) -> bool {
		for keybinding in keybindings.keys.iter() {
			if self.binding_is_tapped(keybinding) {
				return true;
			}
		}
		false
	}
	pub fn is_released(&self, keybindings: &KeyBindings) -> bool {
		for keybinding in keybindings.keys.iter() {
			if self.binding_is_released(keybinding) {
				return true;
			}
		}
		false
	}
}