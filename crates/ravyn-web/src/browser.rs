//! Thin wrappers around DOM APIs that only exist in a real browser. Every
//! function here is a safe no-op when compiled without the `hydrate`
//! feature (i.e. during SSR), so callers don't need to sprinkle `#[cfg]`
//! themselves.
use leptos::ev::DragEvent;

/// Copies dropped files onto the file `<input>` with the given id and
/// submits its enclosing form — so a drop reuses the exact same plain
/// multipart POST as picking a file by hand.
pub fn sync_dropped_files(ev: DragEvent, input_id: &str) {
    ev.prevent_default();

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use web_sys::HtmlInputElement;

        (|| {
            let files = ev.data_transfer()?.files()?;
            let window = web_sys::window()?;
            let document = window.document()?;
            let input = document
                .get_element_by_id(input_id)?
                .dyn_into::<HtmlInputElement>()
                .ok()?;

            input.set_files(Some(&files));
            let _ = input.form()?.request_submit();
            Some(())
        })();
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = input_id;
    }
}

/// Submits the form containing the file `<input>` with the given id — used
/// so picking a file (via click, not drop) uploads immediately instead of
/// requiring a separate button press.
pub fn submit_input_form(input_id: &str) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use web_sys::HtmlInputElement;

        (|| {
            let window = web_sys::window()?;
            let document = window.document()?;
            let input = document
                .get_element_by_id(input_id)?
                .dyn_into::<HtmlInputElement>()
                .ok()?;
            let _ = input.form()?.request_submit();
            Some(())
        })();
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = input_id;
    }
}

/// Builds a synthetic text file out of pasted content and feeds it through
/// the same `<input type=file>` a real drag-drop or file picker would use,
/// then submits its form — reuses the entire upload pipeline (naming,
/// quotas, streaming) exactly as-is, since as far as the server's
/// concerned this is just a file that happens to have been typed instead
/// of picked from disk.
pub fn paste_text_and_submit(input_id: &str, filename: &str, text: &str) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::{JsCast, JsValue};
        use web_sys::{DataTransfer, File, FilePropertyBag, HtmlInputElement};

        (|| {
            let parts = js_sys::Array::of1(&JsValue::from_str(text));
            let options = FilePropertyBag::new();
            options.set_type("text/plain");
            let file = File::new_with_str_sequence_and_options(&parts, filename, &options).ok()?;

            let data_transfer = DataTransfer::new().ok()?;
            data_transfer.items().add_with_file(&file).ok()?;

            let window = web_sys::window()?;
            let document = window.document()?;
            let input = document
                .get_element_by_id(input_id)?
                .dyn_into::<HtmlInputElement>()
                .ok()?;

            input.set_files(Some(&data_transfer.files()?));
            let _ = input.form()?.request_submit();
            Some(())
        })();
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (input_id, filename, text);
    }
}

pub fn copy_to_clipboard(text: &str) {
    #[cfg(feature = "hydrate")]
    {
        if let Some(window) = web_sys::window() {
            let _ = window.navigator().clipboard().write_text(text);
        }
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = text;
    }
}

/// A full browser navigation, not a client-side route change — used after
/// registering, since the register page lives outside the router tree that
/// `DashboardLayout`'s login state reacts to. Reloading picks up the
/// session cookie the server just set and lands on the real, authenticated
/// SSR page rather than trying to fake a client-side transition into it.
pub fn navigate_to(path: &str) {
    #[cfg(feature = "hydrate")]
    {
        if let Some(window) = web_sys::window() {
            let _ = window.location().set_href(path);
        }
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = path;
    }
}
