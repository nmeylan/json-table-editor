use std::time::{Duration, Instant};
#[macro_export]
macro_rules! log {
    () => {
        #[cfg(not(target_arch = "wasm32"))]
        print!("\n")
    };
    ($($arg:tt)*) => {{
        #[cfg(not(target_arch = "wasm32"))]
        println!($($arg)*);
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&std::format_args!($($arg)*).as_str().into());
    }};
}

#[cfg(not(target_arch = "wasm32"))]
pub fn now() -> std::time::Instant {
    Instant::now()
}
#[cfg(target_arch = "wasm32")]
pub fn now() -> crate::compatibility::InstantWrapper {
    InstantWrapper::now()
}

#[cfg(target_arch = "wasm32")]
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Hash)]
pub struct InstantWrapper(Duration);

#[cfg(target_arch = "wasm32")]
impl InstantWrapper {
    #[inline]
    pub fn now() -> Self {
        InstantWrapper(duration_from_f64(_now()))
    }

    #[inline]
    pub fn duration_since(&self, earlier: InstantWrapper) -> Duration {
        assert!(
            earlier.0 <= self.0,
            "`earlier` cannot be later than `self`."
        );
        self.0 - earlier.0
    }

    #[inline]
    pub fn elapsed(&self) -> Duration {
        Self::now().duration_since(*self)
    }

    #[inline]
    pub fn checked_add(&self, duration: Duration) -> Option<InstantWrapper> {
        self.0.checked_add(duration).map(InstantWrapper)
    }

    #[inline]
    pub fn checked_sub(&self, duration: Duration) -> Option<InstantWrapper> {
        self.0.checked_sub(duration).map(InstantWrapper)
    }

    #[inline]
    pub fn checked_duration_since(&self, earlier: InstantWrapper) -> Option<Duration> {
        if earlier.0 > self.0 {
            None
        } else {
            Some(self.0 - earlier.0)
        }
    }

    #[inline]
    pub fn saturating_duration_since(&self, earlier: InstantWrapper) -> Duration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::Add<Duration> for InstantWrapper {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Duration) -> Self {
        InstantWrapper(self.0 + rhs)
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::AddAssign<Duration> for InstantWrapper {
    #[inline]
    fn add_assign(&mut self, rhs: Duration) {
        self.0 += rhs
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::Sub<Duration> for InstantWrapper {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Duration) -> Self {
        InstantWrapper(self.0 - rhs)
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::Sub<InstantWrapper> for InstantWrapper {
    type Output = Duration;

    #[inline]
    fn sub(self, rhs: InstantWrapper) -> Duration {
        self.duration_since(rhs)
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::SubAssign<Duration> for InstantWrapper {
    #[inline]
    fn sub_assign(&mut self, rhs: Duration) {
        self.0 -= rhs
    }
}

#[cfg(target_arch = "wasm32")]
fn duration_from_f64(millis: f64) -> Duration {
    Duration::from_millis(millis.trunc() as u64)
        + Duration::from_nanos((millis.fract() * 1.0e6) as u64)
}

#[cfg(target_arch = "wasm32")]
fn _now() -> f64 {
    let now = {
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsCast;
        web_sys::js_sys::Reflect::get(
            &web_sys::js_sys::global(),
            &wasm_bindgen::JsValue::from_str("performance"),
        )
        .expect("failed to get performance from global object")
        .unchecked_into::<web_sys::Performance>()
        .now()
    };

    now
}
