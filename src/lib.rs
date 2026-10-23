#![allow(static_mut_refs)]
use libc::*;
use poll::*;
use std::ffi::{CStr, CString};
use std::mem::transmute;
use std::str::FromStr;

pub mod adm;
pub mod al;
pub mod card;
pub mod card_redir;
pub mod hook;
pub mod jamma;
pub mod opengl;
pub mod poll;
pub mod res;
pub mod jvsprime;
pub mod ffb;
pub mod steerfix;
pub mod pin8;

#[derive(serde::Deserialize)]
pub struct FileRedirect {
	from: String,
	to: String,
	flags: Option<u32>,
}

#[derive(Debug, serde::Deserialize, PartialEq, Copy, Clone)]
pub enum FullscreenMode {
	#[serde(rename = "exclusive")]
	Exclusive,
	#[serde(rename = "monitor-based")]
	MonitorBased,
	#[serde(rename = "borderless_windowed")]
	BorderlessWindowed,
}

#[derive(serde::Deserialize)]
pub struct Config {
	fullscreen: bool,
	fullscreen_mode: FullscreenMode,
	preserve_aspect_ratio: bool,
	input_emu: bool,
	jvs_prime_device: Option<String>,
	card_emu: bool,
	redir_card: bool,
	card_device: String,
	block_sudo: bool,
	ignore_custom_ioctls: bool,
	dongle: String,
	local_ip: Option<String>,
	deadzone: f32,
	width: u32,
	height: u32,

	debug_menu: bool,
	keyboard_steering_sensitivity: f32,
	keyboard_pedal_sensitivity: f32,
	invert_steering: bool,
	invert_gas: bool,
	invert_brake: bool,

	#[serde(default)]
	skip_steering_error: bool,
	#[serde(default)]
	skip_calibration_check: bool,

	#[serde(default)]
	pin8_device: Option<String>,
	#[serde(default)]
	init_kickback: bool,

	file_redirect: Option<Vec<FileRedirect>>,
}

const fn default_config() -> Config {
	Config {
		fullscreen: false,
		fullscreen_mode: FullscreenMode::Exclusive,
		preserve_aspect_ratio: false,
		input_emu: true,
		jvs_prime_device: None,
		card_emu: true,
		redir_card: false,
		card_device: String::new(),
		block_sudo: true,
		ignore_custom_ioctls: true,
		dongle: String::new(),
		local_ip: None,
		deadzone: 0.01,
		width: 640,
		height: 480,
		debug_menu: false,
		keyboard_steering_sensitivity: 6.0,
		keyboard_pedal_sensitivity: 2.0,
		invert_steering: false,
		invert_gas: false,
		invert_brake: false,
		skip_steering_error: false,
		skip_calibration_check: false,
		pin8_device: None,
		init_kickback: false,
		file_redirect: None,
	}
}

pub struct KeyConfig {
	test: KeyBindings,
	service: KeyBindings,
	card_insert: KeyBindings,
	coin: KeyBindings,

	gear_next: KeyBindings,
	gear_previous: KeyBindings,
	gear_neutral: KeyBindings,
	gear_first: KeyBindings,
	gear_second: KeyBindings,
	gear_third: KeyBindings,
	gear_fourth: KeyBindings,
	gear_fifth: KeyBindings,
	gear_sixth: KeyBindings,
	gear_up: KeyBindings,
	gear_left: KeyBindings,
	gear_down: KeyBindings,
	gear_right: KeyBindings,

	perspective: KeyBindings,
	intrude: KeyBindings,
	gas: KeyBindings,
	brake: KeyBindings,
	wheel_left: KeyBindings,
	wheel_right: KeyBindings,
}

pub static mut CONFIG: Config = default_config();
pub static mut KEYCONFIG: Option<KeyConfig> = None;
pub static mut GAME_VERSION: GameVersion = default_gameversion();

pub extern "C" fn undachi() -> c_int {
	false as c_int
}

pub extern "C" fn adachi() -> c_int {
	true as c_int
}

#[no_mangle]
unsafe extern "C" fn sigaction() -> c_int {
	0
}

#[no_mangle]
unsafe extern "C" fn system(command: *const c_char) -> c_int {
	let cstr = CStr::from_ptr(command);
	let str = cstr.to_str().unwrap();

	if str.contains("date") || str.contains("hwclock") {
		println!("Intercepted system clock command: {}", str);
		return 0;
	}

	if !CONFIG.block_sudo || str.starts_with("find") {
		let command = str.replace("/tmp/", "./tmp/");
		let command = CString::new(command).unwrap();

		let system = CString::new("system").unwrap();
		let system = dlsym(RTLD_NEXT, system.as_ptr());
		let system: extern "C" fn(*const c_char) -> c_int = transmute(system);

		let setenv = CString::new("setenv").unwrap();
		let setenv = dlsym(RTLD_DEFAULT, setenv.as_ptr());
		let setenv: extern "C" fn(*const c_char, *const c_char, c_int) -> c_int = transmute(setenv);

		let preload = CString::new("LD_PRELOAD").unwrap();
		let empty = CString::new("").unwrap();

		setenv(preload.as_ptr(), empty.as_ptr(), 1);
		system(command.as_ptr())
	} else {
		dbg!(str);
		0
	}
}

#[no_mangle]
unsafe extern "C" fn fopen(filename: *const c_char, mode: *const c_char) -> *const () {
	let filename = CStr::from_ptr(filename).to_str().unwrap();
	let filename = if filename.starts_with("/tmp") {
		CString::new(filename.replace("/tmp/", "./tmp/")).unwrap()
	} else {
		CString::new(filename).unwrap()
	};

	let fopen = CString::new("fopen").unwrap();
	let fopen = dlsym(RTLD_NEXT, fopen.as_ptr());
	let fopen: extern "C" fn(*const c_char, *const c_char) -> *const () = transmute(fopen);
	fopen(filename.as_ptr(), mode)
}

#[no_mangle]
unsafe extern "C" fn open(filename: *const c_char, flags: u32) -> *const () {
	let filename = CStr::from_ptr(filename).to_str().unwrap();
	let redirect = &CONFIG
		.file_redirect
		.as_ref()
		.map(|redirects| {
			redirects
				.iter()
				.filter(|redirect| redirect.from == filename)
				.next()
		})
		.flatten();

	let card_dev = card_redir::redirect(filename);

	let filename = if let Some(redirect) = redirect {
		CString::new(redirect.to.clone()).unwrap()
	} else if let Some(dev) = ffb::board_redirect(filename) {
		println!("ffb: {filename} -> {dev}");
		CString::new(dev).unwrap()
	} else if let Some(dev) = card_dev {
		println!("card: {filename} -> {dev}");
		CString::new(dev).unwrap()
	} else if filename.starts_with("/tmp") {
		CString::new(filename.replace("/tmp/", "./tmp/")).unwrap()
	} else {
		CString::new(filename).unwrap()
	};

	let flags = if let Some(redirect) = redirect {
		if let Some(flags) = redirect.flags {
			flags
		} else {
			flags
		}
	} else {
		flags
	};

	let open = CString::new("open").unwrap();
	let open = dlsym(RTLD_NEXT, open.as_ptr());
	let open: extern "C" fn(*const c_char, u32) -> *const () = transmute(open);
	open(filename.as_ptr(), flags)
}

#[no_mangle]
unsafe extern "C" fn ioctl(fd: i32, op: u32, arg: *const c_void) -> i32 {
	let ioctl = CString::new("ioctl").unwrap();
	let ioctl = dlsym(RTLD_NEXT, ioctl.as_ptr());
	let ioctl: extern "C" fn(i32, u32, *const c_void) -> i32 = transmute(ioctl);
	let res = ioctl(fd, op, arg);
	if (op == 0x5463 || op == 0x5464) && CONFIG.ignore_custom_ioctls {
		0
	} else {
		res
	}
}

#[no_mangle]
unsafe extern "C" fn rename(old: *const c_char, new: *const c_char) -> c_int {
	let old = CStr::from_ptr(old).to_str().unwrap();
	let old = old.replace("/tmp/", "./tmp/");
	let old = CString::new(old).unwrap();

	let new = CStr::from_ptr(new).to_str().unwrap();
	let new = new.replace("/tmp/", "./tmp/");
	let new = CString::new(new).unwrap();

	let rename = CString::new("rename").unwrap();
	let rename = dlsym(RTLD_NEXT, rename.as_ptr());
	let rename: extern "C" fn(*const c_char, *const c_char) -> c_int = transmute(rename);
	rename(old.as_ptr(), new.as_ptr())
}

#[no_mangle]
unsafe extern "C" fn _ZNSt13basic_filebufIcSt11char_traitsIcEE4openEPKcSt13_Ios_Openmode(
	this: c_int,
	filename: *const c_char,
	mode: c_int,
) -> *const () {
	if let Ok(filename) = CStr::from_ptr(filename).to_str() {
		let filename = if filename.starts_with("/tmp") {
			CString::new(filename.replace("/tmp/", "./tmp/")).unwrap()
		} else {
			CString::new(filename).unwrap()
		};

		let open =
			CString::new("_ZNSt13basic_filebufIcSt11char_traitsIcEE4openEPKcSt13_Ios_Openmode")
				.unwrap();
		let open = dlsym(RTLD_NEXT, open.as_ptr());
		let open: extern "C" fn(c_int, *const c_char, c_int) -> *const () = transmute(open);
		open(this, filename.as_ptr(), mode)
	} else {
		let open =
			CString::new("_ZNSt13basic_filebufIcSt11char_traitsIcEE4openEPKcSt13_Ios_Openmode")
				.unwrap();
		let open = dlsym(RTLD_NEXT, open.as_ptr());
		let open: extern "C" fn(c_int, *const c_char, c_int) -> *const () = transmute(open);
		open(this, filename, mode)
	}
}

#[no_mangle]
unsafe extern "C" fn _ZNSt14basic_ifstreamIcSt11char_traitsIcEEC1EPKcSt13_Ios_Openmode(
	this: c_int,
	filename: *const c_char,
	mode: c_int,
) -> *const () {
	if let Ok(filename) = CStr::from_ptr(filename).to_str() {
		let filename = if filename.starts_with("/tmp") {
			CString::new(filename.replace("/tmp/", "./tmp/")).unwrap()
		} else if filename.starts_with("/proc/bus/usb/devices") {
			CString::new("./tmp/usb-devices").unwrap()
		} else {
			CString::new(filename).unwrap()
		};

		let open =
			CString::new("_ZNSt14basic_ifstreamIcSt11char_traitsIcEEC1EPKcSt13_Ios_Openmode")
				.unwrap();
		let open = dlsym(RTLD_NEXT, open.as_ptr());
		let open: extern "C" fn(c_int, *const c_char, c_int) -> *const () = transmute(open);
		open(this, filename.as_ptr(), mode)
	} else {
		let open =
			CString::new("_ZNSt14basic_ifstreamIcSt11char_traitsIcEEC1EPKcSt13_Ios_Openmode")
				.unwrap();
		let open = dlsym(RTLD_NEXT, open.as_ptr());
		let open: extern "C" fn(c_int, *const c_char, c_int) -> *const () = transmute(open);
		open(this, filename, mode)
	}
}

static mut HASP_ID: i32 = 1;
unsafe extern "C" fn hasp_login(_: c_int, _: c_int, id: *mut c_int) -> c_int {
	id.write(HASP_ID);
	HASP_ID += 1;
	0
}

unsafe extern "C" fn hasp_size(_: c_int, _: c_int, size: *mut c_int) -> c_int {
	size.write(0xD40);
	0
}

unsafe extern "C" fn hasp_read(
	_: c_int,
	_: c_int,
	offset: c_int,
	length: c_int,
	buffer: *mut u8,
) -> c_int {
	let mut data = [0u8; 0xD40];
	let dongle = CONFIG.dongle.as_bytes();
	let dongle = if dongle.len() < 12 {
		"285013501138".as_bytes()
	} else {
		dongle
	};
	data.as_mut_ptr()
		.offset(0xD00)
		.copy_from_nonoverlapping(dongle.as_ptr(), 12);
	let crc = data
		.iter()
		.take(0x0D)
		.fold(0, |acc: u8, data| acc.wrapping_add(*data));
	data[0x0D] = crc;
	data[0x0F] = std::ops::Not::not(crc);
	let crc = data
		.iter()
		.skip(0xD00)
		.take(0x3E)
		.fold(0, |acc: u8, data| acc.wrapping_add(*data));
	data[0xD3E] = crc;
	data[0xD3F] = std::ops::Not::not(crc);

	buffer.copy_from_nonoverlapping(data.as_ptr().offset(offset as isize), length as usize);
	0
}

static mut ORIGINAL_CL_MAIN: Option<unsafe extern "C" fn(*mut *mut ())> = None;
unsafe extern "C" fn cl_main(log: *mut *mut ()) {
	ORIGINAL_CL_MAIN.unwrap()(log);
	log.write(hook::get_symbol("_ZSt4cout"));
}

fn netmask_for(ip: std::net::Ipv4Addr) -> std::net::Ipv4Addr {
	let fallback = std::net::Ipv4Addr::new(255, 255, 255, 0);
	unsafe {
		let mut ifap: *mut ifaddrs = std::ptr::null_mut();
		if getifaddrs(&mut ifap) != 0 {
			return fallback;
		}

		let mut result = fallback;
		let mut cur = ifap;
		while !cur.is_null() {
			let a = &*cur;
			if !a.ifa_addr.is_null()
				&& !a.ifa_netmask.is_null()
				&& (*a.ifa_addr).sa_family as c_int == AF_INET
			{
				let addr = &*(a.ifa_addr as *const sockaddr_in);
				if addr.sin_addr.s_addr.to_ne_bytes() == ip.octets() {
					let mask = &*(a.ifa_netmask as *const sockaddr_in);
					result = std::net::Ipv4Addr::from(mask.sin_addr.s_addr.to_ne_bytes());
					break;
				}
			}
			cur = a.ifa_next;
		}
		freeifaddrs(ifap);
		result
	}
}

unsafe extern "C" fn get_address(clnet: *mut *mut c_int) -> c_int {
	let local_ip: std::net::Ipv4Addr = match &CONFIG.local_ip {
		Some(s) => match std::net::Ipv4Addr::from_str(s) {
			Ok(ip) => ip,
			Err(_) => {
				eprintln!("Invalid local_ip in config.toml: {s}");
				std::net::Ipv4Addr::new(127, 0, 0, 1)
			}
		},
		None => match local_ip_address::local_ip() {
			Ok(std::net::IpAddr::V4(addr)) => addr,
			_ => {
				eprintln!("Could not determine a local IPv4 address");
				std::net::Ipv4Addr::new(127, 0, 0, 1)
			}
		},
	};

	let local_mask = netmask_for(local_ip);

	let ip = i32::from_be_bytes(local_ip.octets());
	let subnetmask = i32::from_be_bytes(local_mask.octets());

	let net = clnet.byte_offset(0x24).read();
	let net = if net.is_null() {
		clnet.byte_offset(0x1C).read()
	} else {
		net
	};
	net.byte_offset(0x04).write(ip);
	net.byte_offset(0x08).write(subnetmask);
	ip
}

fn forced_iface() -> Option<[u8; 4]> {
	let s = unsafe { CONFIG.local_ip.as_ref()? };
	Some(std::net::Ipv4Addr::from_str(s).ok()?.octets())
}

unsafe fn real_fn(name: &str) -> *mut c_void {
	let name = CString::new(name).unwrap();
	dlsym(RTLD_NEXT, name.as_ptr())
}

#[no_mangle]
unsafe extern "C" fn setsockopt(
	fd: c_int,
	level: c_int,
	name: c_int,
	val: *const c_void,
	len: socklen_t,
) -> c_int {
	let real: extern "C" fn(c_int, c_int, c_int, *const c_void, socklen_t) -> c_int =
		transmute(real_fn("setsockopt"));

	if level == IPPROTO_IP && !val.is_null() {
		if let Some(addr) = forced_iface() {
			if name == IP_MULTICAST_IF && (len == 4 || len >= 12) {
				let mut buf = std::slice::from_raw_parts(val as *const u8, len as usize).to_vec();
				if len == 4 {
					buf.copy_from_slice(&addr);
				} else {
					buf[4..8].copy_from_slice(&addr);
					buf[8..12].copy_from_slice(&0i32.to_ne_bytes());
				}
				return real(fd, level, name, buf.as_ptr() as *const c_void, len);
			}
			if (name == IP_ADD_MEMBERSHIP || name == IP_DROP_MEMBERSHIP) && len >= 8 {
				let mut buf = std::slice::from_raw_parts(val as *const u8, len as usize).to_vec();
				buf[4..8].copy_from_slice(&addr); // imr_interface / imr_address
				if len >= 12 {
					buf[8..12].copy_from_slice(&0i32.to_ne_bytes());
				}
				return real(fd, level, name, buf.as_ptr() as *const c_void, len);
			}
		}
	}
	real(fd, level, name, val, len)
}

#[no_mangle]
unsafe extern "C" fn socket(domain: c_int, ty: c_int, proto: c_int) -> c_int {
	let real: extern "C" fn(c_int, c_int, c_int) -> c_int = transmute(real_fn("socket"));
	let fd = real(domain, ty, proto);

	if fd >= 0 && domain == AF_INET && (ty & 0xF) == SOCK_DGRAM {
		if let Some(addr) = forced_iface() {
			let real_set: extern "C" fn(c_int, c_int, c_int, *const c_void, socklen_t) -> c_int =
				transmute(real_fn("setsockopt"));
			real_set(fd, IPPROTO_IP, IP_MULTICAST_IF, addr.as_ptr() as *const c_void, 4);
		}
	}
	fd
}

// force_self_merge (DX+ / W3P only)
type FnGetPcbId = unsafe extern "C" fn(*const u8) -> i32;
type FnSetJoinState = unsafe extern "C" fn(*mut u8, i32);

static mut GET_PCB_ID: Option<FnGetPcbId> = None;
static mut ORIGINAL_SET_JOIN_STATE: Option<FnSetJoinState> = None;
static mut ORIGINAL_SET_JOIN_STATE_SYNC: Option<FnSetJoinState> = None;

// W3P layout, relative to the clNet pimpl (clNet + 0x24)
const W3P_JOIN_STATE: usize = 0x1602; // u16 per PCB
const W3P_JOIN_STATE_SYNC: usize = 0x1604; // u16 per PCB
const W3P_PCB_STRIDE: usize = 0x30;

unsafe fn mirror_own_state(this: *mut u8, field: usize, state: i32) {
	let Some(get_pcb_id) = GET_PCB_ID else { return };
	let pimpl = *(this.add(0x24) as *const *mut u8);
	if pimpl.is_null() {
		return;
	}
	let own = get_pcb_id(this as *const u8);
	if !(0..4).contains(&own) {
		return;
	}
	let entry = pimpl.add(field + own as usize * W3P_PCB_STRIDE) as *mut u16;
	entry.write_unaligned(state as u16);
}

unsafe extern "C" fn patched_set_join_state(this: *mut u8, state: i32) {
	ORIGINAL_SET_JOIN_STATE.unwrap()(this, state);
	mirror_own_state(this, W3P_JOIN_STATE, state);
}

unsafe extern "C" fn patched_set_join_state_sync(this: *mut u8, state: i32) {
	ORIGINAL_SET_JOIN_STATE_SYNC.unwrap()(this, state);
	mirror_own_state(this, W3P_JOIN_STATE_SYNC, state);
}
// end force_self_merge

#[repr(C)]
pub struct RomInfo {
	name: [u8; 32],
	region: [u8; 32],
	release_type: [u8; 32],
	date: [u8; 32],
	time: [u8; 32],
	revision: c_int,
	revision_name: [u8; 32],
}

impl Into<GameVersion> for &RomInfo {
	fn into(self) -> GameVersion {
		let revision_name = CStr::from_bytes_until_nul(&self.revision_name);
		let Ok(revision_name) = revision_name else {
			return default_gameversion();
		};
		let Ok(revision_name) = revision_name.to_str() else {
			return default_gameversion();
		};

		let mut parts = revision_name.split('-');
		let Some(next) = parts.next() else {
			eprintln!("Unknown game revision: {revision_name}");
			return default_gameversion();
		};
		let major = match next {
			"WM3100" => GameMajor::WM3,
			"W3X100" => GameMajor::W3X,
			"W3P100" => GameMajor::W3P,
			_ => {
				eprintln!("Unknown game revision: {revision_name}");
				return default_gameversion();
			}
		};

		let Some(next) = parts.next() else {
			eprintln!("Unknown game revision: {revision_name}");
			return default_gameversion();
		};
		let region = match next {
			"1" => GameRegion::JP,
			"2" => GameRegion::EN2,
			"3" => GameRegion::EN3,
			"4" => GameRegion::EN4,
			_ => {
				eprintln!("Unknown game revision: {revision_name}");
				return default_gameversion();
			}
		};

		let Some(_) = parts.next() else {
			eprintln!("Unknown game revision: {revision_name}");
			return default_gameversion();
		};
		let Some(_) = parts.next() else {
			eprintln!("Unknown game revision: {revision_name}");
			return default_gameversion();
		};

		let Some(next) = parts.next() else {
			eprintln!("Unknown game revision: {revision_name}");
			return default_gameversion();
		};

		let Some(minor) = next.chars().next() else {
			eprintln!("Unknown game revision: {revision_name}");
			return default_gameversion();
		};

		let minor = match minor {
			'A' => GameMinor::A,
			'B' => GameMinor::B,
			_ => {
				eprintln!("Unknown game revision: {revision_name}");
				return default_gameversion();
			}
		};

		let Ok(revision) = next.chars().skip(1).take(2).collect::<String>().parse() else {
			eprintln!("Unknown game revision: {revision_name}");
			return default_gameversion();
		};

		GameVersion {
			major,
			minor,
			region,
			revision,
		}
	}
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum GameMajor {
	WM3 = 0,
	W3X = 1,
	W3P = 2,
	Unknown,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum GameMinor {
	A = 0,
	B = 1,
	Unknown,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum GameRegion {
	JP = 1,
	EN2 = 2,
	EN3 = 3,
	EN4 = 4,
	Unknown,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct GameVersion {
	major: GameMajor,
	minor: GameMinor,
	region: GameRegion,
	revision: u32,
}

const fn default_gameversion() -> GameVersion {
	GameVersion {
		major: GameMajor::Unknown,
		minor: GameMinor::Unknown,
		region: GameRegion::Unknown,
		revision: 0,
	}
}

type FnN2JvioInit = unsafe extern "C" fn(*const c_char, *mut c_char, c_int) -> c_int;
static mut ORIGINAL_N2JVIO_INIT: Option<FnN2JvioInit> = None;

unsafe extern "C" fn n2jvio_init_hook(a: *const c_char, b: *mut c_char, c: c_int) -> c_int {
	if let Some(dev) = CONFIG.jvs_prime_device.clone().filter(|s| !s.is_empty()) {
		jvsprime::prime(&dev);
	}
	ORIGINAL_N2JVIO_INIT.unwrap()(a, b, c)
}

#[ctor::ctor]
unsafe fn init() {
	match std::fs::read_to_string("config.toml") {
		Ok(text) => match toml::from_str::<Config>(&text) {
			Ok(cfg) => CONFIG = cfg,
			Err(e) => eprintln!("config.toml failed to parse, falling back to defaults:\n{e}"),
		},
		Err(e) => eprintln!("Could not read config.toml ({e}), falling back to defaults"),
	}

	// Really what I should do is implement a custom serde::Deserialize for KeyBindings
	// but serdes documentation is really confusing when it comes to this
	#[derive(serde::Deserialize)]
	#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
	struct KeyConfigTemp {
		test: Vec<String>,
		service: Vec<String>,
		card_insert: Vec<String>,
		coin: Vec<String>,

		gear_next: Vec<String>,
		gear_previous: Vec<String>,
		gear_neutral: Vec<String>,
		gear_first: Vec<String>,
		gear_second: Vec<String>,
		gear_third: Vec<String>,
		gear_fourth: Vec<String>,
		gear_fifth: Vec<String>,
		gear_sixth: Vec<String>,
		gear_up: Vec<String>,
		gear_left: Vec<String>,
		gear_down: Vec<String>,
		gear_right: Vec<String>,

		perspective: Vec<String>,
		intrude: Vec<String>,
		gas: Vec<String>,
		brake: Vec<String>,
		wheel_left: Vec<String>,
		wheel_right: Vec<String>,
	}

	let toml = std::fs::read_to_string("keyconfig.toml")
		.expect("Could not read keyconfig.toml");
	let keyconfig: KeyConfigTemp = toml::from_str(&toml)
		.unwrap_or_else(|e| panic!("keyconfig.toml failed to parse:\n{e}"));
	let keyconfig = KeyConfig {
		test: parse_keybinding(keyconfig.test),
		service: parse_keybinding(keyconfig.service),
		card_insert: parse_keybinding(keyconfig.card_insert),
		coin: parse_keybinding(keyconfig.coin),

		gear_next: parse_keybinding(keyconfig.gear_next),
		gear_previous: parse_keybinding(keyconfig.gear_previous),
		gear_neutral: parse_keybinding(keyconfig.gear_neutral),
		gear_first: parse_keybinding(keyconfig.gear_first),
		gear_second: parse_keybinding(keyconfig.gear_second),
		gear_third: parse_keybinding(keyconfig.gear_third),
		gear_fourth: parse_keybinding(keyconfig.gear_fourth),
		gear_fifth: parse_keybinding(keyconfig.gear_fifth),
		gear_sixth: parse_keybinding(keyconfig.gear_sixth),
		gear_up: parse_keybinding(keyconfig.gear_up),
		gear_left: parse_keybinding(keyconfig.gear_left),
		gear_down: parse_keybinding(keyconfig.gear_down),
		gear_right: parse_keybinding(keyconfig.gear_right),

		perspective: parse_keybinding(keyconfig.perspective),
		intrude: parse_keybinding(keyconfig.intrude),
		gas: parse_keybinding(keyconfig.gas),
		brake: parse_keybinding(keyconfig.brake),
		wheel_left: parse_keybinding(keyconfig.wheel_left),
		wheel_right: parse_keybinding(keyconfig.wheel_right),
	};
	KEYCONFIG = Some(keyconfig);

	hook::hook_symbol("hasp_cleanup", undachi as *const ());
	hook::hook_symbol("hasp_decrypt", undachi as *const ());
	hook::hook_symbol("hasp_encrypt", undachi as *const ());
	hook::hook_symbol("hasp_free", undachi as *const ());
	hook::hook_symbol("hasp_get_rtc", undachi as *const ());
	hook::hook_symbol("hasp_get_sessioninfo", undachi as *const ());
	hook::hook_symbol("hasp_get_size", hasp_size as *const ());
	hook::hook_symbol("hasp_legacy_set_rtc", undachi as *const ());
	hook::hook_symbol("hasp_login", hasp_login as *const ());
	hook::hook_symbol("hasp_logout", undachi as *const ());
	hook::hook_symbol("hasp_read", hasp_read as *const ());
	hook::hook_symbol("hasp_write", undachi as *const ());
	hook::hook_symbol("hasp_get_rtc", undachi as *const ());
	hook::hook_symbol("hasp_hasptime_to_datetime", undachi as *const ());
	hook::hook_symbol("_ZNK6clHasp7isAvailEv", adachi as *const ());

	if CONFIG.local_ip.is_some() || local_ip_address::local_ip().is_ok() {
		hook::hook_symbol("_ZNK5clNet10getAddressEv", get_address as *const ());
	} else {
		hook::hook_symbol("_ZN18clSeqBootNetThread3runEPv", adachi as *const ());
	}

	ORIGINAL_CL_MAIN = Some(transmute(hook::hook_symbol(
		"_ZN6clMainC1Ev",
		cl_main as *const (),
	)));

	adm::init();
	ffb::init();
	let skip_steering = CONFIG.skip_steering_error && !CONFIG.input_emu;
	if skip_steering || CONFIG.skip_calibration_check {
    	steerfix::init(skip_steering, CONFIG.skip_calibration_check);
	}
	pin8::init();
	
	al::load_al_funcs();

	if CONFIG.input_emu {
		jamma::init();
	}else {
		jamma::hook_seq_main();
	}
	if CONFIG.redir_card && CONFIG.card_emu {
		panic!("Invalid configuration: set either redir_card or card_emu, not both.");
	}
	if CONFIG.card_emu {
		card::init();
	}
	if CONFIG.redir_card && CONFIG.card_device.is_empty() {
		println!("card: redir_card is on but card_device is empty, nothing will be redirected");
	}
	if CONFIG.width != 640 || CONFIG.height != 480 {
		res::init();
	}
	if let Some(dev) = CONFIG.jvs_prime_device.clone().filter(|s| !s.is_empty()) {
		let tramp = hook::hook_symbol("n2JvioInit", n2jvio_init_hook as *const ());
		if tramp.is_null() {
			eprintln!("jvs prime: cannot hook n2JvioInit, priming now instead");
			jvsprime::prime(&dev);
		} else {
		ORIGINAL_N2JVIO_INIT = Some(transmute(tramp));
		}
	}

	let rom_info = hook::get_symbol("gRomInfo") as *const RomInfo;
	let rom_info = rom_info.as_ref().unwrap();
	let version = rom_info.into();
	GAME_VERSION = version;

	// force_self_merge: DX+ (W3P) only
	if GAME_VERSION.major == GameMajor::W3P {
		GET_PCB_ID = Some(transmute(hook::get_symbol("_ZNK5clNet8getPcbIdEv")));
		ORIGINAL_SET_JOIN_STATE = Some(transmute(hook::hook_symbol(
			"_ZN5clNet12setJoinStateEi",
			patched_set_join_state as *const (),
		)));
		ORIGINAL_SET_JOIN_STATE_SYNC = Some(transmute(hook::hook_symbol(
			"_ZN5clNet16setJoinStateSyncEi",
			patched_set_join_state_sync as *const (),
		)));
	}

	for plugin in glob::glob("plugins/*.so").unwrap() {
		let plugin_name = plugin.unwrap().to_string_lossy().to_string();
		let plugin = CString::new(plugin_name.clone()).unwrap();
		let plugin = dlopen(plugin.as_ptr(), RTLD_LAZY);
		if plugin.is_null() {
			let error = dlerror();
			let error = CStr::from_ptr(error).to_string_lossy().to_string();
			panic!("{plugin_name} could not be loaded:  {error}");
		}
		let init = CString::new("init").unwrap();
		let init = dlsym(plugin, init.as_ptr());
		if init.is_null() {
			let error = dlerror();
			let error = CStr::from_ptr(error).to_string_lossy().to_string();
			panic!("init does not exist in {plugin_name}: {error}");
		}
		let init: fn(*const GameVersion) = transmute(init);
		init(&version);
	}
}
