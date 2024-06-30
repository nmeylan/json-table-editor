#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;


/// Our handle to the web app from JavaScript.
#[derive(Clone)]
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebHandle {
    /// Installs a panic hook, then returns.
    #[allow(clippy::new_without_default)]
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {

        Self {
            runner: eframe::WebRunner::new(),
        }
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen]
    pub async fn start(&self, canvas_id: &str) -> Result<(), wasm_bindgen::JsValue> {
        self.runner
            .start(
                canvas_id,
                eframe::WebOptions::default(),
                Box::new(|cc| Box::new(MyApp::new(cc))),
            )
            .await
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen]
    pub fn destroy(&self) {
        self.runner.destroy();
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen]
    pub fn example(&self) {
        if let Some(_app) = self.runner.app_mut::<MyApp>() {
            // _app.example();
        }
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen]
    pub fn has_panicked(&self) -> bool {
        self.runner.has_panicked()
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen]
    pub fn panic_message(&self) -> Option<String> {
        self.runner.panic_summary().map(|s| s.message())
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen]
    pub fn panic_callstack(&self) -> Option<String> {
        self.runner.panic_summary().map(|s| s.callstack())
    }
}