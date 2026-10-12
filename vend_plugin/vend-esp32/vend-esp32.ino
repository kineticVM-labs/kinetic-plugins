// Kinetic vend firmware for the existing ESP32 machine.
//
// Flash this file as-is. Same board selection as the sketch already on the
// machine. Same libraries: LiquidCrystal_I2C, Keypad, ESP32Servo.
// Do not change a pin number, a step count, or a servo angle.
//
// The gantry, keypad, LCD, and buzzer are the machine that is already built.
// The serial line is Kinetic's protocol, 115200 8N1, one JSON object per line.
//
// Pi -> ESP32
//   {"id":"<id>","cmd":"ping"}
//   {"id":"<id>","cmd":"capabilities"}
//   {"id":"<id>","cmd":"gpio_read","args":{"pin":1}}
//   {"id":"<id>","cmd":"gpio_write","args":{"pin":1,"value":1}}
//
// gpio_write is a slot command, not an electrical pin:
//   pin 1, value 1   drink-1   servo GPIO 15, gantry 1100 steps
//   pin 2, value 1   drink-2   servo GPIO 4,  gantry 650 steps
//   pin 3, value 1   drink-3   servo GPIO 5,  gantry 250 steps
//   pin 0, any value deny      LCD "Invalid Code", no motor
//   any other pin              error, no motor and no pinMode change
//
// The gpio_write reply is {"id":"<id>","ok":true,"result":"done"} and it is
// sent before the gantry moves. The Pi stops waiting after 5 seconds. The
// move takes longer. Completion is a later event.
//
// ESP32 -> Pi
//   {"event":"message","signal":"boot","text":""}
//   {"event":"message","signal":"keypad","text":"1234"}
//   {"event":"message","signal":"dispensed","text":"drink-1"}
//   {"event":"message","signal":"error","text":"pi_timeout"}
//   {"event":"message","signal":"error","text":"unknown_drink"}
//
// A line that is not a known command never moves a motor.

#include <LiquidCrystal_I2C.h>
#include <Keypad.h>
#include <ESP32Servo.h>
#include <string.h>
#include <stdlib.h>

const unsigned long VERIFY_TIMEOUT_MS = 10000;

LiquidCrystal_I2C lcd(0x27, 16, 2);

const byte ROWS = 4;
const byte COLS = 3;
char keys[ROWS][COLS] = {
  {'1','2','3'},
  {'4','5','6'},
  {'7','8','9'},
  {'*','0','#'}
};
byte rowPins[ROWS] = {13, 12, 14, 27};
byte colPins[COLS] = {26, 25, 33};
Keypad keypad = Keypad(makeKeymap(keys), rowPins, colPins, ROWS, COLS);

const int BUZZER_PIN = 2;
const int BUZZER_FREQ = 1500;
const int BUZZER_DURATION = 75;

const int STEPPER_STEP_PIN = 17;
const int STEPPER_DIR_PIN  = 16;

const int SERVO_BASE_PIN = 18;
const int SERVO_PIN_1    = 15;
const int SERVO_PIN_2    = 4;
const int SERVO_PIN_3    = 5;

Servo servoBase;
Servo servoItem1;
Servo servoItem2;
Servo servoItem3;

const int SERVO_1_2_IDLE_POS = 0;
const int SERVO_1_2_DISPENSE_POS = 90;

const int SERVO_3_IDLE_POS = 90;
const int SERVO_3_DISPENSE_POS = 0;

const int BASE_HOME_POS = 0;
const int BASE_DISPENSE_POS = 180;

const int CHECKOUT_POS = 0;
const int ITEM_1_POS = 1100;
const int ITEM_2_POS = 650;
const int ITEM_3_POS = 250;

int currentStepperPos = 0;

String enteredOtp = "";
bool isVerifying = false;
unsigned long verifyStartedMs = 0;

const int SERIAL_CAP = 192;
char serialBuf[SERIAL_CAP];
int serialLen = 0;
bool serialOverflow = false;

void displayMessage(String line1, String line2, int delay_ms = 0) {
  lcd.clear();
  lcd.setCursor(0, 0);
  lcd.print(line1.substring(0, 16));
  lcd.setCursor(0, 1);
  lcd.print(line2.substring(0, 16));
  if (delay_ms > 0) {
    delay(delay_ms);
  }
}

void resetInputState() {
  enteredOtp = "";
  isVerifying = false;
  displayMessage("Enter OTP:", "****");
}

void sendHost(const char* signal, const char* text) {
  Serial.print(F("{\"event\":\"message\",\"signal\":\""));
  Serial.print(signal);
  Serial.print(F("\",\"text\":\""));
  Serial.print(text);
  Serial.println(F("\"}"));
  Serial.flush();
}

void sendResult(const char* id, const char* result) {
  Serial.print(F("{\"id\":\""));
  Serial.print(id);
  Serial.print(F("\",\"ok\":true,\"result\":\""));
  Serial.print(result);
  Serial.println(F("\"}"));
  Serial.flush();
}

void sendError(const char* id, const char* error) {
  Serial.print(F("{\"id\":\""));
  Serial.print(id);
  Serial.print(F("\",\"ok\":false,\"result\":\"\",\"error\":\""));
  Serial.print(error);
  Serial.println(F("\"}"));
  Serial.flush();
}

void setup() {
  Serial.begin(115200);
  // Bootloader bytes share this UART. Wait so the boot line is its own JSON object.
  delay(250);

  lcd.init();
  lcd.backlight();
  pinMode(BUZZER_PIN, OUTPUT);
  pinMode(STEPPER_STEP_PIN, OUTPUT);
  pinMode(STEPPER_DIR_PIN, OUTPUT);

  ESP32PWM::allocateTimer(0);
  ESP32PWM::allocateTimer(1);
  ESP32PWM::allocateTimer(2);
  ESP32PWM::allocateTimer(3);

  servoBase.attach(SERVO_BASE_PIN);
  servoItem1.attach(SERVO_PIN_1);
  servoItem2.attach(SERVO_PIN_2);
  servoItem3.attach(SERVO_PIN_3);

  servoBase.write(BASE_HOME_POS);
  servoItem1.write(SERVO_1_2_IDLE_POS);
  servoItem2.write(SERVO_1_2_IDLE_POS);
  servoItem3.write(SERVO_3_IDLE_POS);

  sendHost("boot", "");
  resetInputState();
}

void loop() {
  pollSerial();

  if (isVerifying && (millis() - verifyStartedMs > VERIFY_TIMEOUT_MS)) {
    sendHost("error", "pi_timeout");
    displayMessage("System Busy", "Try again", 2500);
    resetInputState();
    return;
  }

  if (isVerifying) return;

  char key = keypad.getKey();

  if (key) {
    tone(BUZZER_PIN, BUZZER_FREQ, BUZZER_DURATION);

    if (key == '#') {
      if (enteredOtp.length() == 4) {
        verifyOTP(enteredOtp);
      } else {
        displayMessage("Invalid OTP", "Must be 4 digits", 2000);
        resetInputState();
      }
    } else if (key == '*') {
      resetInputState();
    } else if (isDigit(key) && enteredOtp.length() < 4) {
      enteredOtp += key;
      lcd.setCursor(enteredOtp.length() - 1, 1);
      lcd.print(key);
    }
  }
}

void verifyOTP(String otp) {
  isVerifying = true;
  verifyStartedMs = millis();
  displayMessage("Verifying OTP...", "");
  sendHost("keypad", otp.c_str());
}

void pollSerial() {
  while (Serial.available()) {
    char c = (char) Serial.read();
    if (c == '\n') {
      if (!serialOverflow && serialLen > 0) {
        serialBuf[serialLen] = '\0';
        handleSerialLine(serialBuf);
      }
      serialLen = 0;
      serialOverflow = false;
      return;
    } else if (c != '\r') {
      if (serialOverflow || serialLen >= SERIAL_CAP - 1) {
        serialOverflow = true;
        serialLen = 0;
      } else {
        serialBuf[serialLen++] = c;
      }
    }
  }
}

void trimLine(char* line) {
  int end = strlen(line);
  while (end > 0 && (line[end - 1] == ' ' || line[end - 1] == '\t' || line[end - 1] == '\r')) {
    line[--end] = '\0';
  }
  int start = 0;
  while (line[start] == ' ' || line[start] == '\t') start++;
  if (start > 0) {
    memmove(line, line + start, strlen(line + start) + 1);
  }
}

void compactJson(const char* in, char* out, int outLen) {
  int j = 0;
  for (int i = 0; in[i] != '\0' && j < outLen - 1; i++) {
    if (in[i] != ' ' && in[i] != '\t') out[j++] = in[i];
  }
  out[j] = '\0';
}

bool copyId(char* out, int outLen, const char* json) {
  const char* p = strstr(json, "\"id\":\"");
  if (!p) {
    out[0] = '\0';
    return false;
  }
  p += 6;
  int i = 0;
  while (i < outLen - 1 && *p && *p != '"') {
    if (*p < '0' || *p > '9') {
      out[0] = '\0';
      return false;
    }
    out[i++] = *p++;
  }
  if (*p != '"' || i == 0) {
    out[0] = '\0';
    return false;
  }
  out[i] = '\0';
  return true;
}

bool hasCmd(const char* json, const char* cmd) {
  char search[40];
  snprintf(search, sizeof(search), "\"cmd\":\"%s\"", cmd);
  return strstr(json, search) != NULL;
}

int parseArg(const char* key, const char* json) {
  char search[16];
  snprintf(search, sizeof(search), "\"%s\":", key);
  const char* p = strstr(json, search);
  if (!p) return -1;
  p += strlen(search);
  if (*p < '0' || *p > '9') return -1;
  long value = strtol(p, NULL, 10);
  if (value > 255) return -1;
  return (int) value;
}

void handleSerialLine(char* line) {
  trimLine(line);
  if (line[0] == '\0') return;
  if (line[0] != '{') return;

  char compact[SERIAL_CAP];
  compactJson(line, compact, sizeof(compact));

  char id[32];
  if (!copyId(id, sizeof(id), compact)) return;

  if (hasCmd(compact, "ping")) {
    sendResult(id, "pong");
    return;
  }

  if (hasCmd(compact, "capabilities")) {
    Serial.print(F("{\"id\":\""));
    Serial.print(id);
    Serial.print(F("\",\"ok\":true,\"result\":\"{\\\"gpio\\\":[0,1,2,3],\\\"led_pin\\\":null,\\\"description\\\":\\\"pin 1 drink-1, pin 2 drink-2, pin 3 drink-3, pin 0 deny\\\"}\"}"));
    Serial.println();
    Serial.flush();
    return;
  }

  if (hasCmd(compact, "gpio_read")) {
    int pin = parseArg("pin", compact);
    if (pin < 0 || pin > 3) {
      sendError(id, "invalid pin");
      return;
    }
    sendResult(id, "0");
    return;
  }

  if (hasCmd(compact, "gpio_write")) {
    int pin = parseArg("pin", compact);
    int value = parseArg("value", compact);
    if (pin < 0 || value < 0) {
      sendError(id, "missing pin or value");
      return;
    }
    if (pin == 0) {
      sendResult(id, "done");
      denyCode();
      return;
    }
    if (pin >= 1 && pin <= 3 && value == 0) {
      sendResult(id, "done");
      return;
    }
    if (pin == 1) {
      sendResult(id, "done");
      runDispense("drink-1");
      return;
    }
    if (pin == 2) {
      sendResult(id, "done");
      runDispense("drink-2");
      return;
    }
    if (pin == 3) {
      sendResult(id, "done");
      runDispense("drink-3");
      return;
    }
    sendError(id, "invalid pin");
    return;
  }

  sendError(id, "unknown command");
}

void denyCode() {
  tone(BUZZER_PIN, 400, 400);
  displayMessage("Invalid Code", "Try again", 3000);
  resetInputState();
}

void runDispense(const char* drinkId) {
  displayMessage("Success!", "Dispensing...");
  dispenseDrink(String(drinkId));
  resetInputState();
}

void dispenseDrink(String drinkId) {
  if (drinkId == "drink-1") {
    dispenseItem(1, ITEM_1_POS, servoItem1);
    sendHost("dispensed", "drink-1");

  } else if (drinkId == "drink-2") {
    dispenseItem(2, ITEM_2_POS, servoItem2);
    sendHost("dispensed", "drink-2");

  } else if (drinkId == "drink-3") {
    dispenseItem(3, ITEM_3_POS, servoItem3);
    sendHost("dispensed", "drink-3");

  } else {
    sendHost("error", "unknown_drink");
    displayMessage("Error:", "Not in Stock");
    delay(2000);
  }
}

void dispenseItem(int itemNumber, int targetSteps, Servo& itemServo) {
  moveStepperTo(targetSteps, "Gantry to Item " + String(itemNumber));
  delay(500);

  if (itemNumber == 3) {
    displayMessage("Dispensing Item", "Servo 3 -> 0");
    itemServo.write(SERVO_3_DISPENSE_POS);
    delay(1000);

    displayMessage("Securing Item...", "Servo 3 -> 90");
    itemServo.write(SERVO_3_IDLE_POS);
    delay(500);

  } else {
    displayMessage("Dispensing Item", "Servo " + String(itemNumber) + " -> 90");
    itemServo.write(SERVO_1_2_DISPENSE_POS);
    delay(1000);

    displayMessage("Securing Item...", "Servo " + String(itemNumber) + " -> 0");
    itemServo.write(SERVO_1_2_IDLE_POS);
    delay(500);
  }

  // Calibrated drop is ITEM_3_POS (250), then the base turns, then home.
  moveStepperTo(ITEM_3_POS, "Gantry to C/Out");

  delay(500);

  displayMessage("Dispensing...", "Base -> 180");
  servoBase.write(BASE_DISPENSE_POS);
  delay(5000);

  displayMessage("Resetting...", "Base -> 0");
  servoBase.write(BASE_HOME_POS);
  delay(1000);
  moveStepperTo(CHECKOUT_POS, "Gantry to Init");

  displayMessage("Complete!", "Item " + String(itemNumber) + " Ready");
  tone(BUZZER_PIN, BUZZER_FREQ, 150);
  delay(200);
  tone(BUZZER_PIN, BUZZER_FREQ, 150);
  delay(2000);
}

void moveStepperTo(int absoluteTargetSteps, String lcdMessage) {
  displayMessage("Moving Gantry...", lcdMessage);

  int stepsToMove = absoluteTargetSteps - currentStepperPos;

  if (stepsToMove > 0) {
    digitalWrite(STEPPER_DIR_PIN, HIGH);
  } else {
    digitalWrite(STEPPER_DIR_PIN, LOW);
  }

  for (int i = 0; i < abs(stepsToMove); i++) {
    digitalWrite(STEPPER_STEP_PIN, HIGH);
    delayMicroseconds(700);
    digitalWrite(STEPPER_STEP_PIN, LOW);
    delayMicroseconds(700);
  }

  currentStepperPos = absoluteTargetSteps;
}
