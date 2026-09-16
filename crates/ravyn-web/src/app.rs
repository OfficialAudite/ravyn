use leptos::prelude::*;
use leptos_meta::{provide_meta_context, Link, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{ParentRoute, Route, Router, Routes},
    ParamSegment, StaticSegment,
};

use crate::admin::AdminPage;
use crate::dashboard::{BrowsePage, DashboardLayout, UploadPage};
use crate::register::RegisterPage;
use crate::settings::SettingsPage;
use crate::shared_folder::SharedFolderPage;

const FAVICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'%3E%3Cpath fill='%235b8dff' d='M2 17.5c2.8-6.2 6.7-9.3 10-9.3s7.2 3.1 10 9.3c-3.1-2.8-6.5-4.1-10-4.1s-6.9 1.3-10 4.1z'/%3E%3C/svg%3E";

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta name="theme-color" content="#0c0d12"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/ravyn-web.css"/>
        <Link rel="icon" href=FAVICON/>
        <Title text="ravyn"/>

        <Router>
            <Routes fallback=|| "page not found">
                <ParentRoute path=StaticSegment("") view=DashboardLayout>
                    <Route path=StaticSegment("") view=BrowsePage/>
                    <Route path=StaticSegment("upload") view=UploadPage/>
                    <Route path=StaticSegment("settings") view=SettingsPage/>
                    <Route path=StaticSegment("admin") view=AdminPage/>
                </ParentRoute>
                <Route path=(StaticSegment("f"), ParamSegment("id")) view=SharedFolderPage/>
                <Route path=StaticSegment("register") view=RegisterPage/>
            </Routes>
        </Router>
    }
}
