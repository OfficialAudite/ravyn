//! Thin wrappers around DOM APIs that only exist in a real browser. Every
//! function here is a safe no-op when compiled without the `hydrate`
//! feature (i.e. during SSR), so callers don't need to sprinkle `#[cfg]`
//! themselves.
use leptos::ev::DragEvent;
use leptos::prelude::*;

/// Copies dropped files onto the file `<input>` with the given id, without
/// submitting anything - the caller decides afterward whether that's a
/// plain form submit or, for a large file, the chunked upload path
/// (`upload_large_files`), the same choice it already has to make for a
/// file picked by hand.
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

/// Pulls the first image out of a clipboard paste (e.g. a screenshot copied
/// straight from the OS, no save-to-disk step first) and copies it onto the
/// file `<input>` with the given id, the same way `sync_dropped_files` does
/// for a drag - the caller still decides afterward whether to submit
/// plainly or chunk it. Returns `false` (and touches nothing) if the paste
/// didn't contain an image, so a plain text paste elsewhere on the page
/// isn't hijacked into an upload attempt.
#[cfg(feature = "hydrate")]
pub fn image_from_clipboard(ev: &web_sys::Event, input_id: &str) -> bool {
    use wasm_bindgen::JsCast;
    use web_sys::{ClipboardEvent, DataTransfer, HtmlInputElement};

    (|| {
        let ev = ev.dyn_ref::<ClipboardEvent>()?;
        let items = ev.clipboard_data()?.items();
        let mut found = None;
        for i in 0..items.length() {
            let item = items.get(i)?;
            if item.kind() == "file" && item.type_().starts_with("image/") {
                found = item.get_as_file().ok().flatten();
                if found.is_some() {
                    break;
                }
            }
        }
        let file = found?;

        let data_transfer = DataTransfer::new().ok()?;
        data_transfer.items().add_with_file(&file).ok()?;

        let window = web_sys::window()?;
        let document = window.document()?;
        let input = document
            .get_element_by_id(input_id)?
            .dyn_into::<HtmlInputElement>()
            .ok()?;

        input.set_files(Some(&data_transfer.files()?));
        Some(())
    })()
    .is_some()
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

/// At or above this, a file uploads through the chunked protocol
/// (`crate::server_fns::init_chunked_upload` and friends) instead of the
/// plain form submit every other file uses - split into chunks small
/// enough that a network blip only costs one chunk's worth of retrying,
/// not the whole file.
#[cfg(feature = "hydrate")]
const LARGE_FILE_THRESHOLD: u32 = 32 * 1024 * 1024;

/// Looks at whatever's currently selected in the `<input>` with the given
/// id. If none of it is large enough to need chunking, does nothing and
/// returns `false` so the caller falls back to a plain form submit
/// instead. If any of it is, uploads the whole selection (small files
/// included, so a mixed batch doesn't need splitting across two
/// mechanisms) through the chunked protocol, one file at a time, and
/// returns `true`.
///
/// `uploading_name` and `progress` are updated as each file starts and
/// each chunk of it lands, for a caller to drive a progress display off
/// of; `error` is set if a file fails after exhausting its retries. A full
/// page navigation back to `/` on success mirrors what the plain form
/// submit's own server-side redirect already does, since nothing else
/// here refreshes the file list.
pub fn upload_large_files(
    input_id: &'static str,
    uploading_name: RwSignal<Option<String>>,
    progress: RwSignal<(u64, u64)>,
    error: RwSignal<Option<String>>,
) -> bool {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use web_sys::HtmlInputElement;

        let Some(list) = (|| {
            let window = web_sys::window()?;
            let document = window.document()?;
            let input = document
                .get_element_by_id(input_id)?
                .dyn_into::<HtmlInputElement>()
                .ok()?;
            input.files()
        })() else {
            return false;
        };

        let count = list.length();
        let has_large = (0..count).any(|i| {
            list.get(i)
                .map(|file| file.size() as u32 >= LARGE_FILE_THRESHOLD)
                .unwrap_or(false)
        });
        if !has_large {
            return false;
        }

        error.set(None);
        leptos::task::spawn_local(async move {
            let mut duplicate_of = None;
            for i in 0..count {
                let Some(file) = list.get(i) else { continue };
                uploading_name.set(Some(file.name()));
                progress.set((0, file.size() as u64));

                match upload_one_large_file(&file, progress).await {
                    Ok(found) => duplicate_of = duplicate_of.or(found),
                    Err(err) => {
                        error.set(Some(err));
                        uploading_name.set(None);
                        return;
                    }
                }
            }

            uploading_name.set(None);
            match duplicate_of {
                Some(id) => navigate_to(&format!("/?duplicate_of={id}")),
                None => navigate_to("/"),
            }
        });

        true
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (input_id, uploading_name, progress, error);
        false
    }
}

#[cfg(feature = "hydrate")]
async fn upload_one_large_file(
    file: &web_sys::File,
    progress: RwSignal<(u64, u64)>,
) -> Result<Option<String>, String> {
    let init =
        crate::server_fns::init_chunked_upload(file.name(), file.type_(), file.size() as i64)
            .await
            .map_err(|err| err.to_string())?;

    use futures::{StreamExt, TryStreamExt};

    // A few chunks in flight at once instead of one at a time: on a link
    // with any real latency, a single request spends much of its time
    // waiting on the round trip rather than actually sending.
    const PARALLEL_CHUNKS: usize = 4;

    let total_size = file.size() as u64;
    let chunk_size = init.chunk_size as u64;
    let part_count = total_size.div_ceil(chunk_size) as i32;
    let upload_id = init.upload_id.as_str();

    futures::stream::iter(1..=part_count)
        .map(Ok)
        .try_for_each_concurrent(PARALLEL_CHUNKS, |part_number| async move {
            let start = (part_number as u64 - 1) * chunk_size;
            let end = (start + chunk_size).min(total_size);
            let blob = file
                .slice_with_f64_and_f64(start as f64, end as f64)
                .map_err(|_| "failed to read part of the file".to_string())?;

            const MAX_ATTEMPTS: u8 = 3;
            let mut attempt = 0;
            loop {
                match upload_one_chunk(upload_id, part_number, &blob, progress).await {
                    Ok(()) => break,
                    Err(err) => {
                        attempt += 1;
                        if attempt >= MAX_ATTEMPTS {
                            return Err(err);
                        }
                        sleep_ms(500).await;
                    }
                }
            }

            Ok(())
        })
        .await?;

    let completed = crate::server_fns::complete_chunked_upload(init.upload_id)
        .await
        .map_err(|err| err.to_string())?;

    Ok(completed.duplicate_of)
}

/// XHR rather than fetch, since fetch has no upload progress events and
/// the bar would otherwise only move once a whole chunk has landed. Bytes
/// are counted into `progress` as they go out, and taken back out again if
/// the attempt fails so a retry doesn't count them twice.
#[cfg(feature = "hydrate")]
async fn upload_one_chunk(
    upload_id: &str,
    part_number: i32,
    blob: &web_sys::Blob,
    progress: RwSignal<(u64, u64)>,
) -> Result<(), String> {
    use std::{cell::Cell, rc::Rc};
    use wasm_bindgen::{closure::Closure, JsCast};

    let xhr =
        web_sys::XmlHttpRequest::new().map_err(|_| "failed to start the upload".to_string())?;
    xhr.open("PATCH", &format!("/upload-chunk/{upload_id}/{part_number}"))
        .map_err(|_| "failed to start the upload".to_string())?;

    let counted = Rc::new(Cell::new(0u64));
    let on_progress = {
        let counted = counted.clone();
        Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(move |event: web_sys::ProgressEvent| {
            let loaded = event.loaded() as u64;
            let delta = loaded.saturating_sub(counted.get());
            counted.set(loaded);
            progress.update(|(sent, _)| *sent += delta);
        })
    };
    if let Ok(upload) = xhr.upload() {
        upload.set_onprogress(Some(on_progress.as_ref().unchecked_ref()));
    }

    let done = js_sys::Promise::new(&mut |resolve, reject| {
        xhr.set_onload(Some(&resolve));
        xhr.set_onerror(Some(&reject));
        xhr.set_onabort(Some(&reject));
    });
    let sent = xhr.send_with_opt_blob(Some(blob));
    let finished = sent.is_ok() && wasm_bindgen_futures::JsFuture::from(done).await.is_ok();
    drop(on_progress);

    let status = xhr.status().unwrap_or(0);
    if finished && (200..300).contains(&status) {
        let rest = blob.size() as u64 - counted.get().min(blob.size() as u64);
        progress.update(|(sent, _)| *sent += rest);
        return Ok(());
    }

    progress.update(|(sent, _)| *sent = sent.saturating_sub(counted.get()));
    if finished {
        Err(format!("server rejected the chunk (status {status})"))
    } else {
        Err("network error".to_string())
    }
}

/// A short pause between chunk retries - not exponential backoff, since a
/// self-hosted instance's network hiccups are usually transient enough
/// that a fixed half-second is plenty.
#[cfg(feature = "hydrate")]
async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
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
