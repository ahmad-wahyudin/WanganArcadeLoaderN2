//! pin8.rs - steering board (STR) support without the JVS I/O board.
//!
//! init_kickback = true   -> hook clKickback::chkSelf(). If the game never called
//!     clKickback::init() (what happens with input_emu = true, because initSystemN2 is a
//!     no-op), call it ourselves first. init() creates and opens the clSerialN2 for port 1
//!     (/dev/ttyM1, so file_redirect / ffb board redirect apply) and sets the "initialized"
//!     flag at this+0x14. Without that flag chkSelf() just returns 1 and bit 7 is masked off.
//!
//! pin8_device = "/dev/ttyUSB1" -> mirror bit 7 of clKickback+0x30 (the byte exec()/manage()
//!     copy to n2jvio[0x1e8]) to an Arduino Nano. Sends '1' / '0' on change and re-sends
//!     every 500 ms (the Nano releases the pin by itself if it hears nothing for 3 s).
//!     bit 7 = 1 starts at "wake" (calibration) and is cleared when the check ends.
//!     The pin polarity is decided in the Nano sketch (PULL_LOW_WHEN_BIT7).
//!
//! UNTESTED. Symbol names are taken from the decompile; check the "not found" messages.
use crate::*;
use std::time::{Duration, Instant};

type FnInit = unsafe extern "C" fn(*mut u8, u32);
type FnChkSelf = unsafe extern "C" fn(*mut u8) -> c_int;
type FnVoid = unsafe extern "C" fn(*mut u8);

static mut KB_INIT: Option<FnInit> = None;
static mut ORIG_CHK_SELF: Option<FnChkSelf> = None;
static mut ORIG_EXEC: Option<FnVoid> = None;
static mut ORIG_MANAGE: Option<FnVoid> = None;
static mut CENTER: *const u32 = std::ptr::null();

static mut FD: c_int = -1;
static mut LAST_BIT: Option<bool> = None;
static mut LAST_TX: Option<Instant> = None;

// ---- init_kickback ---------------------------------------------------------------------

unsafe extern "C" fn chk_self(this: *mut u8) -> c_int {
	if *(this.add(0x14) as *const i32) == 0 {
		let center = if CENTER.is_null() { 0x1ff } else { *CENTER & 0xffff };
		println!("pin8: clKickback was not initialised, calling init(center={center:#x})");
		KB_INIT.unwrap()(this, center);
	}
	ORIG_CHK_SELF.unwrap()(this)
}

// ---- pin 8 mirror ----------------------------------------------------------------------

unsafe fn send_byte(b: u8) {
	libc::write(FD, &b as *const u8 as *const c_void, 1);
}

unsafe fn mirror(this: *mut u8) {
	if FD < 0 {
		return;
	}
	let bit = *this.add(0x30) & 0x80 != 0;
	let changed = LAST_BIT != Some(bit);
	let due = LAST_TX.map_or(true, |t| t.elapsed() >= Duration::from_millis(500));
	if changed || due {
		send_byte(if bit { b'1' } else { b'0' });
		LAST_TX = Some(Instant::now());
		if changed {
			println!("pin8: bit7 = {}", bit as u8);
			LAST_BIT = Some(bit);
		}
	}
}

unsafe extern "C" fn exec_hook(this: *mut u8) {
	ORIG_EXEC.unwrap()(this);
	mirror(this);
}

unsafe extern "C" fn manage_hook(this: *mut u8) {
	ORIG_MANAGE.unwrap()(this);
	mirror(this);
}

unsafe fn open_nano(path: &str) -> c_int {
	let cpath = CString::new(path).unwrap();
	let fd = libc::open(cpath.as_ptr(), O_RDWR | O_NOCTTY | O_NONBLOCK);
	if fd < 0 {
		eprintln!("pin8: cannot open {path}");
		return -1;
	}
	let mut t: termios = std::mem::zeroed();
	tcgetattr(fd, &mut t);
	cfmakeraw(&mut t);
	cfsetispeed(&mut t, B115200);
	cfsetospeed(&mut t, B115200);
	t.c_cflag |= CLOCAL | CREAD | CS8;
	t.c_cflag &= !(PARENB | CSTOPB);
	tcsetattr(fd, TCSANOW, &t);
	tcflush(fd, TCIOFLUSH);
	// opening the port resets a Nano (DTR); wait for the bootloader to finish
	usleep(2_000_000);
	println!("pin8: Arduino on {path}");
	fd
}

pub unsafe fn init() {
	if CONFIG.init_kickback {
		KB_INIT = Some(transmute(hook::get_symbol("_ZN10clKickback4initEj")));
		CENTER = hook::get_symbol("_ZN10clKickback17sm_default_centerE") as *const u32;
		let t = hook::hook_symbol("_ZN10clKickback7chkSelfEv", chk_self as *const ());
		if t.is_null() || KB_INIT.is_none() {
			eprintln!("pin8: clKickback::chkSelf / init not found, init_kickback inactive");
		} else {
			ORIG_CHK_SELF = Some(transmute(t));
			println!("pin8: init_kickback active");
		}
	}

	if let Some(dev) = CONFIG.pin8_device.clone().filter(|s| !s.is_empty()) {
		FD = open_nano(&dev);
		if FD >= 0 {
			send_byte(b'0');
			let e = hook::hook_symbol("_ZN10clKickback4execEv", exec_hook as *const ());
			let m = hook::hook_symbol("_ZN10clKickback6manageEv", manage_hook as *const ());
			if e.is_null() {
				eprintln!("pin8: cannot hook clKickback::exec");
			} else {
				ORIG_EXEC = Some(transmute(e));
			}
			if m.is_null() {
				eprintln!("pin8: cannot hook clKickback::manage");
			} else {
				ORIG_MANAGE = Some(transmute(m));
			}
		}
	}
}
