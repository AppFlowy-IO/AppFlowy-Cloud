use crate::wasm_trace;

pub struct WASMLogger;

impl log::Log for WASMLogger {
  fn enabled(&self, metadata: &log::Metadata) -> bool {
    metadata.level() <= log::Level::Debug
  }

  fn log(&self, record: &log::Record) {
    let level = record.level();
    let target = record.target();
    let args = format!("{}", record.args());
    wasm_trace(&level.to_string(), target, &args);
  }

  fn flush(&self) {}
}

impl Default for WASMLogger {
  fn default() -> Self {
    Self
  }
}

static WASM_LOGGER: WASMLogger = WASMLogger;

pub fn init_logger() {
  log::set_logger(&WASM_LOGGER).unwrap();
  log::set_max_level(log::LevelFilter::Debug);
}
