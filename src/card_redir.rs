use crate::*;

/// The game opens the card reader at /dev/ttyM0. Send only that path to card_device.
pub fn redirect(path: &str) -> Option<&'static str> {
	let c = unsafe { &CONFIG };
	if c.redir_card && !c.card_device.is_empty() && path == "/dev/ttyM0" {
		Some(c.card_device.as_str())
	} else {
		None
	}
}