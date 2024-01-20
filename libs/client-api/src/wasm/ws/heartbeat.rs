use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::JsCast;


#[derive(Debug)]
pub struct Heartbeat {
    interval_id: Option<i32>,
    interval_ms: i32,
}

impl Heartbeat {
    pub fn new(interval_ms: i32) -> Heartbeat {
        Heartbeat {
            interval_id: None,
            interval_ms,
        }
    }

    pub fn start<F>(&mut self, mut callback: F)
        where
            F: FnMut() + 'static,
    {
        let window = web_sys::window().expect("should have a Window");

        let closure = Closure::wrap(Box::new(move || {
            callback();
        }) as Box<dyn FnMut()>);

        let interval_id = window.set_interval_with_closure_and_timeout_and_arguments_0(
            closure.as_ref().unchecked_ref(),
            self.interval_ms,
        ).expect("should register `setInterval`");

        closure.forget(); // must forget, otherwise it will be dropped immediately
        self.interval_id = Some(interval_id);
    }

    pub fn stop(&mut self) {
        if let Some(interval_id) = self.interval_id {
            let window = web_sys::window().expect("should have a Window");
            window.clear_interval_with_handle(interval_id);
            self.interval_id = None;
        }
    }
}