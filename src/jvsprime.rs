use crate::*;
use std::time::{Duration, Instant};

fn encode(node: u8, payload: &[u8]) -> Vec<u8> {
	let mut body = vec![node, payload.len() as u8 + 1];
	body.extend_from_slice(payload);
	let sum = body.iter().fold(0u8, |a, b| a.wrapping_add(*b));
	body.push(sum);
	let mut out = vec![0xE0];
	for b in body {
		if b == 0xE0 || b == 0xD0 {
			out.push(0xD0);
			out.push(b - 1);
		} else {
			out.push(b);
		}
	}
	out
}

unsafe fn send(fd: c_int, node: u8, payload: &[u8]) {
	let pkt = encode(node, payload);
	libc::write(fd, pkt.as_ptr() as *const c_void, pkt.len());
	tcdrain(fd);
}

unsafe fn read_byte(fd: c_int, deadline: Instant) -> Option<u8> {
	loop {
		let mut b = 0u8;
		if libc::read(fd, &mut b as *mut u8 as *mut c_void, 1) == 1 {
			return Some(b);
		}
		if Instant::now() >= deadline {
			return None;
		}
		usleep(500);
	}
}

unsafe fn read_esc(fd: c_int, deadline: Instant) -> Option<u8> {
	let b = read_byte(fd, deadline)?;
	if b == 0xD0 {
		Some(read_byte(fd, deadline)?.wrapping_add(1))
	} else {
		Some(b)
	}
}

/// Payload (status + reports) of the next valid packet addressed to the host.
unsafe fn read_reply(fd: c_int, timeout: Duration) -> Option<Vec<u8>> {
	let deadline = Instant::now() + timeout;
	loop {
		while read_byte(fd, deadline)? != 0xE0 {}
		let node = read_esc(fd, deadline)?;
		let len = read_esc(fd, deadline)? as usize;
		if len == 0 {
			continue;
		}
		let mut data = Vec::with_capacity(len);
		for _ in 0..len {
			data.push(read_esc(fd, deadline)?);
		}
		let sum = data.pop()?;
		let calc = data
			.iter()
			.fold(node.wrapping_add(len as u8), |a, b| a.wrapping_add(*b));
		if calc == sum && node == 0 {
			return Some(data);
		}
		// bad checksum, or an echo of our own packet: keep looking
	}
}

unsafe fn request(fd: c_int, node: u8, payload: &[u8]) -> Option<Vec<u8>> {
	for _ in 0..3 {
		tcflush(fd, TCIFLUSH);
		send(fd, node, payload);
		if let Some(r) = read_reply(fd, Duration::from_millis(100)) {
			if r.first() == Some(&1) {
				return Some(r);
			}
		}
	}
	None
}

pub unsafe fn prime(path: &str) {
	let t0 = Instant::now();
	let cpath = CString::new(path).unwrap();
	let fd = libc::open(cpath.as_ptr(), O_RDWR | O_NOCTTY | O_NONBLOCK);
	if fd < 0 {
		eprintln!("jvs prime: cannot open {path}");
		return;
	}

	let mut t: termios = std::mem::zeroed();
	tcgetattr(fd, &mut t);
	cfmakeraw(&mut t);
	cfsetispeed(&mut t, B115200);
	cfsetospeed(&mut t, B115200);
	t.c_cflag |= CLOCAL | CREAD | CS8;
	t.c_cflag &= !(PARENB | CSTOPB);
	t.c_cc[VMIN] = 0;
	t.c_cc[VTIME] = 0;
	tcsetattr(fd, TCSANOW, &t);
	tcflush(fd, TCIOFLUSH);

	// two back-to-back resets, 100 ms, flush.
	send(fd, 0xFF, &[0xF0, 0xD9]);
	send(fd, 0xFF, &[0xF0, 0xD9]);
	usleep(100_000);
	tcflush(fd, TCIFLUSH);

	if request(fd, 0xFF, &[0xF1, 1]).is_none() {
		eprintln!("jvs prime: SETADDR got no reply on {path}");
		close(fd);
		return;
	}
	println!("jvs prime: SETADDR ok");

	let steps: [(&str, u8); 5] = [
		("ident", 0x10),
		("cmdrev", 0x11),
		("jvsrev", 0x12),
		("comver", 0x13),
		("features", 0x14),
	];
	for (name, cmd) in steps {
		match request(fd, 1, &[cmd]) {
			Some(r) if cmd == 0x10 => {
				let s = String::from_utf8_lossy(r.get(2..).unwrap_or(&[]))
					.trim_end_matches('\0')
					.to_string();
				println!("jvs prime: ident \"{s}\"");
			}
			Some(_) => println!("jvs prime: {name} ok"),
			None => eprintln!("jvs prime: {name} got no reply"),
		}
	}
	close(fd);
	println!("jvs prime: done in {} ms", t0.elapsed().as_millis());
}