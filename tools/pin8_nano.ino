// pin8_nano.ino - drives the steering board J101 pin 8 for WanganArcadeLoader (pin8.rs)
//
// The loader sends '1' / '0' = bit 7 of clKickback+0x30 (1 = steering check running).
// If nothing arrives for 3 s the pin is released, so a crashed game cannot hold it.
//
// Wiring: Nano GND -> board GND. D2 -> J101 pin 8 ONLY after measuring the voltage on
// pin 8 on the real cabinet. A Nano is 5 V logic; if the line is higher than that, or
// pulled up to something else, use a transistor/optocoupler instead of a direct wire.

const uint8_t PIN = 2;

// true : bit 7 = 1 pulls the pin LOW
// false: bit 7 = 1 drives it HIGH
const bool PULL_LOW_WHEN_BIT7 = true;

// true : open drain. LOW is driven, "high" = pin released (input). Use when the board
//        has its own pull-up on pin 8. false: push-pull, the Nano drives HIGH itself.
const bool OPEN_DRAIN = true;

const unsigned long TIMEOUT_MS = 3000;

bool bit7 = false;
unsigned long lastRx = 0;

void drive(bool low) {
  if (low) {
    digitalWrite(PIN, LOW);
    pinMode(PIN, OUTPUT);
  } else if (OPEN_DRAIN) {
    pinMode(PIN, INPUT);          // released
  } else {
    digitalWrite(PIN, HIGH);
    pinMode(PIN, OUTPUT);
  }
}

void apply() {
  bool low = (bit7 == PULL_LOW_WHEN_BIT7);
  drive(low);
}

void setup() {
  Serial.begin(115200);
  bit7 = false;
  apply();
  lastRx = millis();
}

void loop() {
  while (Serial.available()) {
    int c = Serial.read();
    if (c == '1' || c == '0') {
      bit7 = (c == '1');
      lastRx = millis();
      apply();
    }
  }
  if (millis() - lastRx > TIMEOUT_MS) {
    bit7 = false;                 // idle state
    apply();
  }
}
