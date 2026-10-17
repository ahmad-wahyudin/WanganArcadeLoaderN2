//! steerfix.rs - two independent fixes for the steering-board error shown with the real N2 stack
//! (input_emu = false).
//!
//! skip_steering_error = true   -> hook clKickback::getPCBError()
//!     While the test-menu "steering power" is OFF, always report "no steering error", so
//!     seqMain / JammaPCBErrorCheck() never enter the PCB-error state (the E20 screen).
//!     With steering power ON the original behaviour is kept.
//!
//! skip_calibration_check = true -> hook clHandleError::setError()
//!     The boot thread (clSeqBootSteerDeviceThread::run) calls setError("E20") when the steering
//!     self-check fails, whatever the steering power setting. It sets its failure flag
//!     (thread+0x18c) first, and clSeqBootThread::run then waits forever for the error screen to
//!     take over. Just dropping setError therefore hangs the boot. This hook drops the E20 AND
//!     clears that flag, so the boot carries on into the game.
//!
//! Both are only installed from lib.rs when input_emu is false.
use crate::*;

type FnGetMotorPower = unsafe extern "C" fn(*const c_void) -> c_int;
type FnGetPcbError = unsafe extern "C" fn(*const c_void) -> c_int;
type FnSetError = unsafe extern "C" fn(*mut c_void, *const c_char);

static mut GET_MOTOR_POWER: Option<FnGetMotorPower> = None;
static mut CL_MAIN_INSTANCE: *const *const u8 = std::ptr::null();
static mut ORIGINAL_GET_PCB_ERROR: Option<FnGetPcbError> = None;
static mut ORIGINAL_SET_ERROR: Option<FnSetError> = None;

/// Test-menu "steering power". None = can't tell (too early / symbols missing).
unsafe fn steering_power_on() -> Option<bool> {
	let get = GET_MOTOR_POWER?;
	if CL_MAIN_INSTANCE.is_null() {
		return None;
	}
	let instance = *CL_MAIN_INSTANCE;
	if instance.is_null() {
		return None;
	}
	// same chain the boot thread uses: sm_instance -> clMain at +0x1c -> clTestMode* at +0x2c
	let test_mode = *(instance.add(0x1c + 0x2c) as *const *const c_void);
	if test_mode.is_null() {
		return None;
	}
	Some(get(test_mode) != 0)
}

// ---- skip_steering_error -------------------------------------------------------------------

unsafe extern "C" fn get_pcb_error(this: *const c_void) -> c_int {
	if steering_power_on() == Some(false) {
		return 0; // steering power off: never report a steering board error
	}
	ORIGINAL_GET_PCB_ERROR.unwrap()(this)
}

unsafe fn init_skip_steering_error() {
	let gmp = hook::get_symbol("_ZNK10clTestMode13getMotorPowerEv");
	if gmp.is_null() {
		eprintln!("steerfix: clTestMode::getMotorPower not found, skip_steering_error inactive");
	} else {
		GET_MOTOR_POWER = Some(transmute(gmp));
	}
	CL_MAIN_INSTANCE = hook::get_symbol("_ZN11teSingletonI10teSequenceI6clMainEE11sm_instanceE")
		as *const *const u8;

	let t = hook::hook_symbol("_ZN10clKickback11getPCBErrorEv", get_pcb_error as *const ());
	if t.is_null() {
		eprintln!("steerfix: cannot hook clKickback::getPCBError");
	} else {
		ORIGINAL_GET_PCB_ERROR = Some(transmute(t));
	}
}

// ---- skip_calibration_check ----------------------------------------------------------------

unsafe extern "C" fn set_error(this: *mut c_void, code: *const c_char) {
	if !code.is_null() && CStr::from_ptr(code).to_bytes() == b"E20" {
		// `this` is the clHandleError member at +0xc0 of clSeqBootSteerDeviceThread; the
		// thread's failure flag is the byte at +0x18c, i.e. this + 0xcc. Clear it, otherwise
		// clSeqBootThread::run spins forever waiting for an error screen that never comes.
		*(this as *mut u8).add(0x18c - 0xc0) = 0;
		println!("steerfix: skipped steering calibration error E20");
		return;
	}
	ORIGINAL_SET_ERROR.unwrap()(this, code);
}

unsafe fn init_skip_calibration_check() {
	let t = hook::hook_symbol("_ZN13clHandleError8setErrorEPKc", set_error as *const ());
	if t.is_null() {
		eprintln!("steerfix: cannot hook clHandleError::setError");
	} else {
		ORIGINAL_SET_ERROR = Some(transmute(t));
	}
}

pub unsafe fn init(skip_steering_error: bool, skip_calibration_check: bool) {
	if skip_steering_error {
		init_skip_steering_error();
	}
	if skip_calibration_check {
		init_skip_calibration_check();
	}
}