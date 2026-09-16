// The dashboard's view tree (folder sidebar + filter bar + file grid + token
// form, each with several nested components) nests deep enough in Leptos's
// typed-view system to blow the compiler's default query recursion limit —
// this raises it rather than restructuring the component tree just to
// satisfy the compiler.
#![recursion_limit = "256"]

pub mod admin;
pub mod app;
pub mod browser;
pub mod dashboard;
pub mod format;
pub mod icons;
pub mod register;
pub mod server_fns;
pub mod settings;
pub mod shared_folder;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
